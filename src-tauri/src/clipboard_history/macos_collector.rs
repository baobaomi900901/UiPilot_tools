//! Internal observer-to-history pipeline. No production host starts it.
//! Stop/Drop retire only this observer; explicit history shutdown is separate.
use std::sync::Arc;
use super::{macos_history::{HistoryError, IngestReport, MacHistory},
    macos_observer::{CaptureFailure, MacClipboardObserver, ObservationLease, StartError, StopOutcome}};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CollectorError { Capture(CaptureFailure), History(HistoryError) }

pub(super) struct MacHistoryCollector { observer: MacClipboardObserver }
impl MacHistoryCollector {
    pub(super) fn start(
        history: Arc<MacHistory>,
        mut report: impl FnMut(Result<IngestReport, CollectorError>) + Send + 'static,
    ) -> Result<Self, StartError> {
        let binding_history = Arc::clone(&history);
        let observer = MacClipboardObserver::start_bound(
            move || binding_history.prepare_read().map_err(|_| ()),
            move |event| report(event.map_err(CollectorError::Capture)
                .and_then(|event| history.ingest(event).map_err(CollectorError::History))),
        )?;
        Ok(Self { observer })
    }
    pub(super) fn request_stop(&self) { self.observer.request_stop(); }
    pub(super) fn stop(&self) -> StopOutcome { self.observer.stop() }
    pub(super) fn lease(&self) -> ObservationLease { self.observer.lease() }

    #[cfg(test)]
    fn start_with<S: super::macos_observer::Source + 'static>(
        history: Arc<MacHistory>,
        factory: impl FnOnce() -> Result<S, StartError> + Send + 'static,
        clock: impl FnMut() -> Result<String, ()> + Send + 'static,
        mut report: impl FnMut(Result<IngestReport, CollectorError>) + Send + 'static,
    ) -> Result<Self, StartError> {
        let binding_history = Arc::clone(&history);
        let observer = MacClipboardObserver::start_with_binding(factory, std::time::Duration::from_millis(2),
            move || binding_history.prepare_read().map_err(|_| ()), clock,
            move |event| report(event.map_err(CollectorError::Capture)
                .and_then(|event| history.ingest(event).map_err(CollectorError::History))))?;
        Ok(Self { observer })
    }
}

#[cfg(test)]
#[path = "macos_collector_tests.rs"]
mod tests;
