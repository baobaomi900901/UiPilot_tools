//! Targeted Win+V cleanup for one temporary selection copy. Never clears history.
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use super::SelectionCaptureError;

pub(super) struct WindowsHistory {
    baseline: Option<HashSet<String>>,
}

trait HistoryBackend {
    type Item;
    /// None means history is disabled. Baseline enumeration reads IDs only.
    fn query(
        &mut self,
        deadline: Instant,
        owned: &mut dyn FnMut() -> bool,
    ) -> Result<Option<Vec<(String, Self::Item)>>, ()>;
    fn matches(
        &mut self,
        item: &Self::Item,
        text: &str,
        deadline: Instant,
        owned: &mut dyn FnMut() -> bool,
    ) -> Result<bool, ()>;
    fn delete(&mut self, item: &Self::Item) -> Result<(), ()>;
    fn pause(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

fn begin_with(
    backend: &mut impl HistoryBackend,
    deadline: Instant,
) -> Result<WindowsHistory, SelectionCaptureError> {
    if Instant::now() >= deadline {
        return Err(SelectionCaptureError::HistoryUnavailable);
    }
    let baseline = backend
        .query(deadline, &mut || true)
        .map_err(|_| SelectionCaptureError::HistoryUnavailable)?
        .map(|items| items.into_iter().map(|(id, _)| id).collect());
    if Instant::now() >= deadline {
        return Err(SelectionCaptureError::HistoryUnavailable);
    }
    Ok(WindowsHistory { baseline })
}

fn cleanup_with(
    history: &WindowsHistory,
    backend: &mut impl HistoryBackend,
    text: &str,
    owned: &mut dyn FnMut() -> bool,
    observation_end: Instant,
    deadline: Instant,
) -> Result<(), SelectionCaptureError> {
    let Some(baseline) = &history.baseline else {
        return Ok(());
    };
    let error = SelectionCaptureError::HistoryCleanup;
    let mut seen_matches = HashSet::new();
    let mut observed = false;
    let mut last_candidate = None;
    loop {
        if !owned() || Instant::now() >= deadline {
            return Err(error);
        }
        if Instant::now() >= observation_end {
            if !observed {
                return Err(error);
            }
            break;
        }
        // Observation duration is separate from the async operation deadline.
        // Scheduler overshoot ends a healthy observation, not a failed query.
        let items = backend.query(deadline, owned).map_err(|_| error)?;
        if !owned() || Instant::now() >= deadline {
            return Err(error);
        }
        let Some(items) = items else {
            return Ok(());
        };
        let mut candidate = None;
        for (id, item) in items {
            if !owned() || Instant::now() >= deadline {
                return Err(error);
            }
            // Never ask for old contents, even when they have the same text.
            if baseline.contains(&id) {
                continue;
            }
            if backend
                .matches(&item, text, deadline, owned)
                .map_err(|_| error)?
            {
                seen_matches.insert(id);
                if seen_matches.len() > 1 {
                    return Err(error);
                }
                candidate = Some(item);
            }
        }
        observed = true;
        last_candidate = candidate;
        if !owned() || Instant::now() >= deadline {
            return Err(error);
        }
        let remaining = observation_end.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        backend.pause(remaining.min(Duration::from_millis(40)));
    }
    // A later snapshot may reveal a second matching ID. Wait through the
    // observation window before deleting the last snapshot's unique candidate.
    if !owned() || Instant::now() >= deadline {
        return Err(error);
    }
    if let Some(item) = last_candidate {
        backend.delete(&item).map_err(|_| error)?;
    }
    Ok(())
}

fn wait_ready<T>(
    mut ready: impl FnMut() -> Result<bool, ()>,
    result: impl FnOnce() -> Result<T, ()>,
    mut cancel: impl FnMut(),
    owned: &mut dyn FnMut() -> bool,
    deadline: Instant,
) -> Result<T, ()> {
    loop {
        if !owned() || Instant::now() >= deadline {
            cancel();
            return Err(());
        }
        match ready() {
            Ok(true) => {
                if !owned() || Instant::now() >= deadline {
                    cancel();
                    return Err(());
                }
                return result();
            }
            Ok(false) => {
                // WinRT clipboard calls run in an STA. Dispatch its messages
                // while awaiting completion; sleeping alone can starve COM.
                #[cfg(windows)]
                native::pump_messages();
                std::thread::sleep(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(5)),
                );
            }
            Err(()) => {
                cancel();
                return Err(());
            }
        }
    }
}

#[cfg(windows)]
impl WindowsHistory {
    /// Runs on the capture worker; each call balances its own WinRT apartment.
    pub(super) fn begin(deadline: Instant) -> Result<Self, SelectionCaptureError> {
        let _apartment =
            native::Apartment::new().map_err(|_| SelectionCaptureError::HistoryUnavailable)?;
        begin_with(&mut native::Backend, deadline)
    }

    /// Call after conditional clipboard restoration. `still_owned` must verify
    /// the restored sequence/owner and input guards, not its text. Cleanup is
    /// compensating work for an admitted copy and may outlive dispatch cancellation.
    pub(super) fn cleanup(
        &self,
        text: &str,
        mut still_owned: impl FnMut() -> bool,
        deadline: Instant,
    ) -> Result<(), SelectionCaptureError> {
        if self.baseline.is_none() {
            return Ok(());
        }
        let _apartment =
            native::Apartment::new().map_err(|_| SelectionCaptureError::HistoryCleanup)?;
        // Leave time for the deletion and the transaction's final checks.
        let end = deadline
            .checked_sub(Duration::from_millis(50))
            .ok_or(SelectionCaptureError::HistoryCleanup)?
            .min(Instant::now() + Duration::from_millis(650));
        let operation_deadline = deadline
            .checked_sub(Duration::from_millis(50))
            .ok_or(SelectionCaptureError::HistoryCleanup)?;
        cleanup_with(
            self,
            &mut native::Backend,
            text,
            &mut still_owned,
            end,
            operation_deadline,
        )
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use windows::{
        ApplicationModel::DataTransfer::{
            Clipboard, ClipboardHistoryItem, ClipboardHistoryItemsResultStatus, StandardDataFormats,
        },
        Win32::{
            System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_SINGLETHREADED},
            UI::WindowsAndMessaging::{
                DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
            },
        },
    };

    fn diagnosed<T>(stage: &'static str, result: windows::core::Result<T>) -> Result<T, ()> {
        result.map_err(|error| super::super::native::trace(stage, i64::from(error.code().0)))
    }

    pub(super) fn pump_messages() {
        // Bound each dispatch batch so ownership/deadline checks keep running.
        // This is the private capture worker, never the app's UI thread.
        unsafe {
            let mut message = MSG::default();
            for _ in 0..32 {
                if !PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    break;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    pub(super) struct Apartment;
    impl Apartment {
        pub(super) fn new() -> Result<Self, ()> {
            // UIA's MTA scope has already ended before this fallback begins.
            // IsHistoryEnabled rejects an MTA with RO_E_UNSUPPORTED_FROM_MTA.
            diagnosed("history_apartment", unsafe {
                RoInitialize(RO_INIT_SINGLETHREADED)
            })?;
            Ok(Self)
        }
    }
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { RoUninitialize() };
        }
    }

    pub(super) struct Backend;
    impl HistoryBackend for Backend {
        type Item = ClipboardHistoryItem;
        fn query(
            &mut self,
            deadline: Instant,
            owned: &mut dyn FnMut() -> bool,
        ) -> Result<Option<Vec<(String, Self::Item)>>, ()> {
            check(owned, deadline)?;
            if !diagnosed("history_enabled", Clipboard::IsHistoryEnabled())? {
                return Ok(None);
            }
            check(owned, deadline)?;
            let operation = diagnosed("history_query_start", Clipboard::GetHistoryItemsAsync())?;
            let result = wait_ready(
                || status(operation.Status().map_err(|_| ())?.0),
                || diagnosed("history_query_result", operation.GetResults()),
                || {
                    let _ = operation.Cancel();
                },
                owned,
                deadline,
            )?;
            check(owned, deadline)?;
            let result_status = diagnosed("history_query_status", result.Status())?;
            match result_status {
                ClipboardHistoryItemsResultStatus::ClipboardHistoryDisabled => return Ok(None),
                ClipboardHistoryItemsResultStatus::Success => {}
                _ => {
                    super::super::native::trace("history_query_denied", i64::from(result_status.0));
                    return Err(());
                }
            }
            let items = result.Items().map_err(|_| ())?;
            let count = items.Size().map_err(|_| ())?;
            // Defensive resource bound, including pinned records.
            if count > 512 {
                return Err(());
            }
            let mut snapshot = Vec::with_capacity(count as usize);
            for index in 0..count {
                check(owned, deadline)?;
                let item = items.GetAt(index).map_err(|_| ())?;
                let id = item.Id().map_err(|_| ())?.to_string();
                if id.is_empty() {
                    return Err(());
                }
                snapshot.push((id, item));
            }
            Ok(Some(snapshot))
        }
        fn matches(
            &mut self,
            item: &Self::Item,
            text: &str,
            deadline: Instant,
            owned: &mut dyn FnMut() -> bool,
        ) -> Result<bool, ()> {
            check(owned, deadline)?;
            let content = item.Content().map_err(|_| ())?;
            if !content
                .Contains(&StandardDataFormats::Text().map_err(|_| ())?)
                .map_err(|_| ())?
            {
                return Ok(false);
            }
            check(owned, deadline)?;
            let operation = content.GetTextAsync().map_err(|_| ())?;
            let copied = wait_ready(
                || status(operation.Status().map_err(|_| ())?.0),
                || operation.GetResults().map_err(|_| ()),
                || {
                    let _ = operation.Cancel();
                },
                owned,
                deadline,
            )?;
            check(owned, deadline)?;
            // Compare UTF-16 directly, avoiding another copy of history text.
            Ok(copied.iter().copied().eq(text.encode_utf16()))
        }
        fn delete(&mut self, item: &Self::Item) -> Result<(), ()> {
            Clipboard::DeleteItemFromHistory(item)
                .map_err(|_| ())?
                .then_some(())
                .ok_or(())
        }
    }
    fn check(owned: &mut dyn FnMut() -> bool, deadline: Instant) -> Result<(), ()> {
        (owned() && Instant::now() < deadline)
            .then_some(())
            .ok_or(())
    }
    fn status(value: i32) -> Result<bool, ()> {
        // WinRT AsyncStatus: Started=0, Completed=1, Canceled=2, Error=3.
        match value {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, collections::VecDeque};

    #[cfg(windows)]
    #[test]
    fn windows_history_apartment_is_sta_after_uia_mta_teardown() {
        use windows::Win32::System::Com::{
            CoGetApartmentType, CoInitializeEx, CoUninitialize, APTTYPE, APTTYPEQUALIFIER,
            APTTYPE_MAINSTA, APTTYPE_STA, COINIT_MULTITHREADED,
        };
        // Exercise the real thread setup without accessing clipboard contents.
        std::thread::spawn(|| unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap();
            CoUninitialize();
            let _apartment = native::Apartment::new().unwrap();
            let mut kind = APTTYPE::default();
            let mut qualifier = APTTYPEQUALIFIER::default();
            CoGetApartmentType(&mut kind, &mut qualifier).unwrap();
            assert!(
                kind == APTTYPE_STA || kind == APTTYPE_MAINSTA,
                "history requires STA, got {kind:?}"
            );
        })
        .join()
        .unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_history_async_wait_dispatches_worker_messages() {
        use windows::Win32::{
            Foundation::HWND,
            UI::WindowsAndMessaging::{KillTimer, SetTimer},
        };
        thread_local! { static TICKED: Cell<bool> = const { Cell::new(false) }; }
        unsafe extern "system" fn tick(_: HWND, _: u32, _: usize, _: u32) {
            TICKED.set(true);
        }
        std::thread::spawn(|| unsafe {
            let _apartment = native::Apartment::new().unwrap();
            let timer = SetTimer(None, 0, 10, Some(tick));
            assert_ne!(timer, 0);
            let result = wait_ready(
                || Ok(TICKED.get()),
                || Ok(()),
                || {},
                &mut || true,
                Instant::now() + Duration::from_millis(300),
            );
            KillTimer(None, timer).unwrap();
            assert_eq!(result, Ok(()));
        })
        .join()
        .unwrap();
    }

    type Snapshot = Result<Option<Vec<(String, String)>>, ()>;
    struct Fake {
        snapshots: VecDeque<Snapshot>,
        current: Snapshot,
        read: Vec<String>,
        deleted: Vec<String>,
        delete_error: bool,
        pause_delay: Option<Duration>,
    }
    fn entries(items: &[(&str, &str)]) -> Snapshot {
        Ok(Some(
            items
                .iter()
                .map(|(id, text)| (id.to_string(), text.to_string()))
                .collect(),
        ))
    }
    impl Fake {
        fn new(snapshots: impl IntoIterator<Item = Snapshot>) -> Self {
            Self {
                snapshots: snapshots.into_iter().collect(),
                current: entries(&[]),
                read: vec![],
                deleted: vec![],
                delete_error: false,
                pause_delay: None,
            }
        }
    }
    impl HistoryBackend for Fake {
        type Item = (String, String);
        fn query(
            &mut self,
            _: Instant,
            _: &mut dyn FnMut() -> bool,
        ) -> Result<Option<Vec<(String, Self::Item)>>, ()> {
            if let Some(next) = self.snapshots.pop_front() {
                self.current = next;
            }
            self.current.clone().map(|items| {
                items.map(|items| {
                    items
                        .into_iter()
                        .map(|(id, text)| (id.clone(), (id, text)))
                        .collect()
                })
            })
        }
        fn matches(
            &mut self,
            item: &Self::Item,
            text: &str,
            _: Instant,
            _: &mut dyn FnMut() -> bool,
        ) -> Result<bool, ()> {
            self.read.push(item.0.clone());
            Ok(item.1 == text)
        }
        fn delete(&mut self, item: &Self::Item) -> Result<(), ()> {
            if self.delete_error {
                return Err(());
            }
            self.deleted.push(item.0.clone());
            Ok(())
        }
        fn pause(&mut self, _: Duration) {
            std::thread::sleep(self.pause_delay.unwrap_or(Duration::from_millis(1)));
        }
    }
    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(1)
    }
    fn clean(
        history: &WindowsHistory,
        fake: &mut Fake,
        owned: &mut dyn FnMut() -> bool,
    ) -> Result<(), SelectionCaptureError> {
        cleanup_with(
            history,
            fake,
            "selection",
            owned,
            Instant::now() + Duration::from_millis(12),
            deadline(),
        )
    }
    #[test]
    fn windows_history_preserves_old_same_text_and_other_new_content() {
        let mut fake = Fake::new([
            entries(&[("old", "selection")]),
            entries(&[
                ("old", "selection"),
                ("new", "selection"),
                ("other", "user copy"),
            ]),
        ]);
        let history = begin_with(&mut fake, deadline()).unwrap();
        assert!(fake.read.is_empty());
        assert_eq!(clean(&history, &mut fake, &mut || true), Ok(()));
        assert_eq!(fake.deleted, ["new"]);
        assert!(!fake.read.iter().any(|id| id == "old"));
    }
    #[test]
    fn windows_history_ambiguous_new_matches_are_not_deleted() {
        let mut fake = Fake::new([
            entries(&[]),
            entries(&[("a", "selection"), ("b", "selection")]),
        ]);
        let history = begin_with(&mut fake, deadline()).unwrap();
        assert_eq!(
            clean(&history, &mut fake, &mut || true),
            Err(SelectionCaptureError::HistoryCleanup)
        );
        assert!(fake.deleted.is_empty());
    }
    #[test]
    fn windows_history_disabled_is_noop() {
        let mut fake = Fake::new([Ok(None)]);
        let history = begin_with(&mut fake, deadline()).unwrap();
        assert_eq!(clean(&history, &mut fake, &mut || false), Ok(()));
        assert!(fake.deleted.is_empty());
    }
    #[test]
    fn windows_history_denied_baseline_stops_before_copy() {
        let mut fake = Fake::new([Err(())]);
        assert!(matches!(
            begin_with(&mut fake, deadline()),
            Err(SelectionCaptureError::HistoryUnavailable)
        ));
    }
    #[test]
    fn windows_history_no_new_record_is_valid_after_observation() {
        let mut fake = Fake::new([entries(&[("old", "selection")])]);
        let history = begin_with(&mut fake, deadline()).unwrap();
        assert_eq!(clean(&history, &mut fake, &mut || true), Ok(()));
        assert!(fake.deleted.is_empty());
    }
    #[test]
    fn windows_history_delayed_record_is_seen() {
        let mut fake = Fake::new([
            entries(&[]),
            entries(&[]),
            entries(&[("late", "selection")]),
        ]);
        let history = begin_with(&mut fake, deadline()).unwrap();
        assert_eq!(clean(&history, &mut fake, &mut || true), Ok(()));
        assert_eq!(fake.deleted, ["late"]);
    }
    #[test]
    fn windows_history_ownership_lost_while_waiting_preserves_new_items() {
        let mut fake = Fake::new([entries(&[]), entries(&[("new", "selection")])]);
        let history = begin_with(&mut fake, deadline()).unwrap();
        let checks = Cell::new(0);
        let mut owned = || {
            checks.set(checks.get() + 1);
            checks.get() < 3
        };
        assert_eq!(
            clean(&history, &mut fake, &mut owned),
            Err(SelectionCaptureError::HistoryCleanup)
        );
        assert!(fake.deleted.is_empty());
    }
    #[test]
    fn windows_history_query_and_delete_failures_are_reported() {
        let mut fake = Fake::new([entries(&[]), Err(())]);
        let history = begin_with(&mut fake, deadline()).unwrap();
        assert_eq!(
            clean(&history, &mut fake, &mut || true),
            Err(SelectionCaptureError::HistoryCleanup)
        );
        let mut fake = Fake::new([entries(&[]), entries(&[("new", "selection")])]);
        fake.delete_error = true;
        let history = begin_with(&mut fake, deadline()).unwrap();
        assert_eq!(
            clean(&history, &mut fake, &mut || true),
            Err(SelectionCaptureError::HistoryCleanup)
        );
    }
    #[test]
    fn windows_history_async_query_timeout_is_bounded_and_cancelled() {
        let cancelled = Cell::new(false);
        let result = wait_ready(
            || Ok(false),
            || Ok(()),
            || cancelled.set(true),
            &mut || true,
            Instant::now() + Duration::from_millis(8),
        );
        assert_eq!(result, Err(()));
        assert!(cancelled.get());
    }
    #[test]
    fn windows_history_async_query_checks_ownership_before_results() {
        let read = Cell::new(false);
        assert_eq!(
            wait_ready(
                || Ok(true),
                || {
                    read.set(true);
                    Ok(())
                },
                || {},
                &mut || false,
                deadline()
            ),
            Err(())
        );
        assert!(!read.get());
    }
    #[test]
    fn windows_history_scheduler_overshoot_finishes_healthy_observation() {
        let mut fake = Fake::new([entries(&[]), entries(&[])]);
        fake.pause_delay = Some(Duration::from_millis(20));
        let history = begin_with(&mut fake, deadline()).unwrap();
        assert_eq!(clean(&history, &mut fake, &mut || true), Ok(()));
        assert!(fake.deleted.is_empty());
    }
    #[test]
    fn windows_history_later_second_match_prevents_early_deletion() {
        let mut fake = Fake::new([
            entries(&[]),
            entries(&[("a", "selection")]),
            entries(&[("a", "selection"), ("b", "selection")]),
        ]);
        let history = begin_with(&mut fake, deadline()).unwrap();
        assert_eq!(
            clean(&history, &mut fake, &mut || true),
            Err(SelectionCaptureError::HistoryCleanup)
        );
        assert!(fake.deleted.is_empty());
    }
}
