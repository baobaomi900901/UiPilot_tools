use std::sync::Mutex;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TransferTarget {
    Find {
        transfer_id: u64,
    },
    Plugin {
        plugin_id: String,
        submission_token: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MainWindowSnapshot {
    pub(crate) visible: bool,
    pub(crate) focused: bool,
    pub(crate) always_on_top: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TransferLease {
    epoch: u64,
    target: TransferTarget,
}

#[derive(Default)]
pub(crate) struct MainWindowTransferCoordinator {
    core: Mutex<TransferCore>,
}

#[derive(Default)]
struct TransferCore {
    next_epoch: u64,
    current: Option<ActiveTransfer>,
    pending_main_blurs: usize,
}

struct ActiveTransfer {
    lease: TransferLease,
    snapshot: MainWindowSnapshot,
    expected_main_blur: bool,
}

impl MainWindowTransferCoordinator {
    pub(crate) fn begin(
        &self,
        target: TransferTarget,
        snapshot: MainWindowSnapshot,
    ) -> Option<TransferLease> {
        let mut core = self.core.lock().ok()?;
        core.next_epoch = core.next_epoch.checked_add(1)?;
        let lease = TransferLease {
            epoch: core.next_epoch,
            target,
        };
        let expected_main_blur = snapshot.focused;
        core.current = Some(ActiveTransfer {
            lease: lease.clone(),
            snapshot,
            expected_main_blur,
        });
        Some(lease)
    }

    pub(crate) fn is_current(&self, lease: &TransferLease) -> bool {
        self.core
            .lock()
            .ok()
            .and_then(|core| core.current.as_ref().map(|current| current.lease == *lease))
            .unwrap_or(false)
    }

    pub(crate) fn current_lease(&self, target: &TransferTarget) -> Option<TransferLease> {
        self.core
            .lock()
            .ok()?
            .current
            .as_ref()
            .filter(|current| current.lease.target == *target)
            .map(|current| current.lease.clone())
    }
    pub(crate) fn consume_expected_main_blur(&self) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        if let Some(current) = core.current.as_mut() {
            if std::mem::take(&mut current.expected_main_blur) {
                return true;
            }
        }
        if core.pending_main_blurs > 0 {
            core.pending_main_blurs -= 1;
            true
        } else {
            false
        }
    }
    pub(crate) fn commit(&self, lease: &TransferLease) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        if core
            .current
            .as_ref()
            .is_some_and(|current| current.lease == *lease)
        {
            let current = core.current.take().expect("validated current transfer");
            if current.expected_main_blur {
                core.pending_main_blurs = core.pending_main_blurs.saturating_add(1);
            }
            true
        } else {
            false
        }
    }
    pub(crate) fn rollback(&self, lease: &TransferLease) -> Option<MainWindowSnapshot> {
        let mut core = self.core.lock().ok()?;
        if !core
            .current
            .as_ref()
            .is_some_and(|current| current.lease == *lease)
        {
            return None;
        }
        core.current.take().map(|current| current.snapshot)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(topmost: bool) -> MainWindowSnapshot {
        MainWindowSnapshot {
            visible: true,
            focused: true,
            always_on_top: topmost,
        }
    }

    #[test]
    fn plugin_and_find_share_one_lease_and_stale_failure_cannot_rollback_new_owner() {
        let coordinator = MainWindowTransferCoordinator::default();
        let plugin = coordinator
            .begin(
                TransferTarget::Plugin {
                    plugin_id: "com.example.a".into(),
                    submission_token: "submit-a".into(),
                },
                snapshot(true),
            )
            .unwrap();
        assert!(coordinator.is_current(&plugin));
        assert!(coordinator.consume_expected_main_blur());
        assert!(!coordinator.consume_expected_main_blur());

        let find = coordinator
            .begin(TransferTarget::Find { transfer_id: 7 }, snapshot(false))
            .unwrap();
        assert!(!coordinator.is_current(&plugin));
        assert_eq!(coordinator.rollback(&plugin), None);
        assert_eq!(coordinator.rollback(&find), Some(snapshot(false)));
    }

    #[test]
    fn only_current_owner_can_commit_or_restore_captured_main_state() {
        let coordinator = MainWindowTransferCoordinator::default();
        let first = coordinator
            .begin(TransferTarget::Find { transfer_id: 1 }, snapshot(true))
            .unwrap();
        let second = coordinator
            .begin(
                TransferTarget::Plugin {
                    plugin_id: "com.example.b".into(),
                    submission_token: "submit-b".into(),
                },
                snapshot(false),
            )
            .unwrap();
        assert!(!coordinator.commit(&first));
        assert!(coordinator.commit(&second));
        assert!(!coordinator.is_current(&second));
        assert!(coordinator.consume_expected_main_blur());
        assert!(!coordinator.consume_expected_main_blur());
    }
}
