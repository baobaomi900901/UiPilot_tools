use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

/// Odd epochs own an active selection. Even epochs admit new show requests.
#[derive(Debug, Default)]
pub(crate) struct DisplayGate(AtomicU64);

pub(crate) struct DisplayLease {
    gate: Arc<DisplayGate>,
    epoch: u64,
}

impl DisplayGate {
    pub fn ticket(&self) -> Option<u64> {
        let value = self.0.load(Ordering::Acquire);
        (value % 2 == 0).then_some(value)
    }
    pub fn accepts(&self, ticket: u64) -> bool {
        self.ticket() == Some(ticket)
    }
    pub fn acquire(self: &Arc<Self>) -> Option<DisplayLease> {
        let old = self
            .0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                (value % 2 == 0)
                    .then(|| value.checked_add(2).map(|_| value + 1))
                    .flatten()
            })
            .ok()?;
        Some(DisplayLease {
            gate: Arc::clone(self),
            epoch: old + 1,
        })
    }
}

impl Drop for DisplayLease {
    fn drop(&mut self) {
        let _ = self.gate.0.compare_exchange(
            self.epoch,
            self.epoch + 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_retires_queued_shows_and_never_replays_them() {
        let gate = Arc::new(DisplayGate::default());
        let queued = gate.ticket().unwrap();
        let lease = gate.acquire().unwrap();
        assert!(gate.ticket().is_none());
        assert!(gate.acquire().is_none());
        assert!(!gate.accepts(queued));
        drop(lease);
        assert!(!gate.accepts(queued));
        assert!(gate.accepts(gate.ticket().unwrap()));
    }
}
