use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LaunchResult {
    Started { pid: u32, creation_time: u64 },
    UacCancelled,
    LaunchFailed,
    LaunchExpired,
}

#[derive(Clone, Default)]
pub(crate) struct LaunchCoordinator {
    pending: Arc<AtomicBool>,
}

#[derive(Debug)]
pub(crate) struct LaunchReservation {
    pending: Arc<AtomicBool>,
    returned: bool,
}

impl LaunchCoordinator {
    pub(crate) fn reserve(&self) -> Result<LaunchReservation, &'static str> {
        self.pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "LaunchAlreadyPending")?;
        Ok(LaunchReservation {
            pending: Arc::clone(&self.pending),
            returned: false,
        })
    }
    pub(crate) fn is_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire)
    }
}
impl LaunchReservation {
    pub(crate) fn cancel_wait(&mut self) {}
    pub(crate) fn complete(mut self, result: LaunchResult) -> LaunchResult {
        self.returned = true;
        result
    }
}
impl Drop for LaunchReservation {
    fn drop(&mut self) {
        if self.returned {
            self.pending.store(false, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_one_reservation_until_native_call_returns() {
        let coordinator = LaunchCoordinator::default();
        let reservation = coordinator.reserve().unwrap();
        assert!(coordinator.is_pending());
        assert_eq!(coordinator.reserve().unwrap_err(), "LaunchAlreadyPending");
        drop(reservation);
        assert!(coordinator.is_pending());
    }
    #[test]
    fn completion_releases_slot_after_success_or_failure() {
        let coordinator = LaunchCoordinator::default();
        let mut reservation = coordinator.reserve().unwrap();
        reservation.cancel_wait();
        assert_eq!(
            reservation.complete(LaunchResult::LaunchExpired),
            LaunchResult::LaunchExpired
        );
        assert!(!coordinator.is_pending());
        assert!(coordinator.reserve().is_ok());
    }
    #[test]
    fn result_preserves_exact_created_identity() {
        let coordinator = LaunchCoordinator::default();
        let result = coordinator
            .reserve()
            .unwrap()
            .complete(LaunchResult::Started {
                pid: 7,
                creation_time: 11,
            });
        assert_eq!(
            result,
            LaunchResult::Started {
                pid: 7,
                creation_time: 11
            }
        );
    }
}
