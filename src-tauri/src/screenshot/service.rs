use std::sync::{Arc, Mutex};

use serde::Serialize;

use super::{
    geometry::{BudgetReservation, PixelBudget},
    model::{ScreenshotError, ScreenshotMode},
};

#[derive(Debug)]
pub(crate) struct CapturedImage {
    pub width: u32,
    pub height: u32,
    png: Vec<u8>,
    _memory: BudgetReservation,
}

impl CapturedImage {
    pub fn png(&self) -> &[u8] {
        &self.png
    }
    /// Callers retain an encoding workspace reservation until this adopts the buffer.
    pub fn from_encoded(
        width: u32,
        height: u32,
        png: Vec<u8>,
        budget: &PixelBudget,
    ) -> Result<Self, ScreenshotError> {
        if width == 0 || height == 0 || png.is_empty() {
            return Err(ScreenshotError::InvalidPixels);
        }
        let memory = budget.reserve(png.capacity())?;
        Ok(Self {
            width,
            height,
            png,
            _memory: memory,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Origin {
    Launcher,
    Hotkey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CaptureTicket {
    pub session_id: u64,
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Owner {
    pub session_id: u64,
    pub generation: u64,
    pub window_instance: u64,
}

impl Owner {
    pub fn ticket(self) -> CaptureTicket {
        CaptureTicket {
            session_id: self.session_id,
            generation: self.generation,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Begin {
    Capture(CaptureTicket),
    FocusPreview,
    Busy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureFinish {
    Preview,
    Cancelled(Origin),
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Phase {
    Preparing,
    Selecting,
    Preview,
    Copying,
    Saving,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputKind {
    Copy,
    Save,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputFinish {
    Preview,
    Closed,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ServiceError {
    Unavailable,
    Expired,
    Busy,
    Exhausted,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    pub session_id: String,
    pub generation: String,
    pub window_instance: String,
    pub revision: String,
    pub mode: ScreenshotMode,
    pub phase: Phase,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub error: Option<String>,
}

#[derive(Debug)]
pub(crate) struct OutputLease {
    pub image: Arc<CapturedImage>,
    pub owner: Owner,
    operation_revision: u64,
}

#[derive(Debug)]
struct Session {
    ticket: CaptureTicket,
    origin: Origin,
    mode: ScreenshotMode,
    phase: Phase,
    image: Option<Arc<CapturedImage>>,
    window_instance: Option<u64>,
    close_pending: bool,
    error: Option<String>,
    output_revision: Option<u64>,
}

impl Session {
    fn owns(&self, owner: Owner) -> bool {
        self.ticket == owner.ticket() && self.window_instance == Some(owner.window_instance)
    }
}

#[derive(Debug, Default)]
struct State {
    next_session: u64,
    revision: u64,
    current: Option<Session>,
    terminal: bool,
}

impl State {
    fn bump(&mut self) -> Result<u64, ServiceError> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(ServiceError::Exhausted)?;
        Ok(self.revision)
    }
}

#[derive(Debug, Default)]
pub(crate) struct ScreenshotService {
    state: Mutex<State>,
    pub budget: PixelBudget,
}

impl ScreenshotService {
    pub fn begin(&self, mode: ScreenshotMode, origin: Origin) -> Result<Begin, ServiceError> {
        let mut state = self.state.lock().map_err(|_| ServiceError::Unavailable)?;
        if state.terminal {
            return Err(ServiceError::Unavailable);
        }
        if let Some(session) = &state.current {
            return Ok(if session.phase == Phase::Preview {
                Begin::FocusPreview
            } else {
                Begin::Busy
            });
        }
        let id = state
            .next_session
            .checked_add(1)
            .ok_or(ServiceError::Exhausted)?;
        state.bump()?;
        state.next_session = id;
        let ticket = CaptureTicket {
            session_id: id,
            generation: 1,
        };
        state.current = Some(Session {
            ticket,
            origin,
            mode,
            phase: Phase::Preparing,
            image: None,
            window_instance: None,
            close_pending: false,
            error: None,
            output_revision: None,
        });
        Ok(Begin::Capture(ticket))
    }

    pub fn current_ticket(&self) -> Option<CaptureTicket> {
        self.state.lock().ok()?.current.as_ref().map(|s| s.ticket)
    }

    pub fn selecting(&self, ticket: CaptureTicket) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if !state
            .current
            .as_ref()
            .is_some_and(|s| s.ticket == ticket && s.phase == Phase::Preparing)
        {
            return false;
        }
        if state.bump().is_err() {
            return false;
        }
        state.current.as_mut().unwrap().phase = Phase::Selecting;
        true
    }

    pub fn finish_capture(
        &self,
        ticket: CaptureTicket,
        result: Result<Option<Arc<CapturedImage>>, String>,
    ) -> CaptureFinish {
        let Ok(mut state) = self.state.lock() else {
            return CaptureFinish::Stale;
        };
        if !state.current.as_ref().is_some_and(|s| {
            s.ticket == ticket && matches!(s.phase, Phase::Preparing | Phase::Selecting)
        }) {
            return CaptureFinish::Stale;
        }
        if state.bump().is_err() {
            return CaptureFinish::Stale;
        }
        let session = state.current.as_mut().unwrap();
        match result {
            Ok(Some(image)) => {
                session.image = Some(image);
                session.error = None;
            }
            Ok(None) => session.error = None,
            Err(error) => session.error = Some(error),
        }
        if session.image.is_some() {
            session.phase = Phase::Preview;
            CaptureFinish::Preview
        } else {
            let origin = session.origin;
            state.current = None;
            CaptureFinish::Cancelled(origin)
        }
    }

    /// Finish an output or explicit cancellation without reopening any preview.
    pub fn finish_selection_hidden(&self, ticket: CaptureTicket) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if !state
            .current
            .as_ref()
            .is_some_and(|session| session.ticket == ticket && session.phase == Phase::Selecting)
            || state.bump().is_err()
        {
            return false;
        }
        state.current = None;
        true
    }

    pub fn bind_window(&self, ticket: CaptureTicket, instance: u64) -> Result<Owner, ServiceError> {
        if instance == 0 {
            return Err(ServiceError::Expired);
        }
        let mut state = self.state.lock().map_err(|_| ServiceError::Unavailable)?;
        let session = state.current.as_ref().ok_or(ServiceError::Expired)?;
        if session.ticket != ticket
            || session.phase != Phase::Preview
            || session.window_instance.is_some_and(|i| i != instance)
        {
            return Err(ServiceError::Expired);
        }
        state.bump()?;
        state.current.as_mut().unwrap().window_instance = Some(instance);
        Ok(Owner {
            session_id: ticket.session_id,
            generation: ticket.generation,
            window_instance: instance,
        })
    }

    pub fn owner(&self, instance: u64) -> Result<Owner, ServiceError> {
        let state = self.state.lock().map_err(|_| ServiceError::Unavailable)?;
        let session = state.current.as_ref().ok_or(ServiceError::Expired)?;
        if session.window_instance != Some(instance) {
            return Err(ServiceError::Expired);
        }
        Ok(Owner {
            session_id: session.ticket.session_id,
            generation: session.ticket.generation,
            window_instance: instance,
        })
    }

    pub fn snapshot(&self, instance: u64) -> Result<Snapshot, ServiceError> {
        let state = self.state.lock().map_err(|_| ServiceError::Unavailable)?;
        let s = state.current.as_ref().ok_or(ServiceError::Expired)?;
        if s.window_instance != Some(instance) {
            return Err(ServiceError::Expired);
        }
        Ok(Snapshot {
            session_id: s.ticket.session_id.to_string(),
            generation: s.ticket.generation.to_string(),
            window_instance: instance.to_string(),
            revision: state.revision.to_string(),
            mode: s.mode,
            phase: s.phase,
            width: s.image.as_ref().map(|i| i.width),
            height: s.image.as_ref().map(|i| i.height),
            error: s.error.clone(),
        })
    }

    pub fn mode(&self, ticket: CaptureTicket) -> Result<ScreenshotMode, ServiceError> {
        let state = self.state.lock().map_err(|_| ServiceError::Unavailable)?;
        state
            .current
            .as_ref()
            .filter(|s| s.ticket == ticket)
            .map(|s| s.mode)
            .ok_or(ServiceError::Expired)
    }

    pub fn image(&self, owner: Owner) -> Result<Arc<CapturedImage>, ServiceError> {
        let state = self.state.lock().map_err(|_| ServiceError::Unavailable)?;
        let session = state
            .current
            .as_ref()
            .filter(|s| s.owns(owner))
            .ok_or(ServiceError::Expired)?;
        session.image.clone().ok_or(ServiceError::Busy)
    }

    pub fn retake(&self, owner: Owner) -> Result<CaptureTicket, ServiceError> {
        let mut state = self.state.lock().map_err(|_| ServiceError::Unavailable)?;
        let session = state
            .current
            .as_ref()
            .filter(|s| s.owns(owner))
            .ok_or(ServiceError::Expired)?;
        if session.phase != Phase::Preview {
            return Err(ServiceError::Busy);
        }
        let generation = session
            .ticket
            .generation
            .checked_add(1)
            .ok_or(ServiceError::Exhausted)?;
        state.bump()?;
        let session = state.current.as_mut().unwrap();
        session.ticket.generation = generation;
        session.phase = Phase::Preparing;
        session.error = None;
        Ok(session.ticket)
    }

    pub fn begin_output(
        &self,
        owner: Owner,
        kind: OutputKind,
    ) -> Result<OutputLease, ServiceError> {
        let mut state = self.state.lock().map_err(|_| ServiceError::Unavailable)?;
        let session = state
            .current
            .as_ref()
            .filter(|s| s.owns(owner))
            .ok_or(ServiceError::Expired)?;
        if session.phase != Phase::Preview {
            return Err(ServiceError::Busy);
        }
        let image = session.image.clone().ok_or(ServiceError::Unavailable)?;
        let revision = state.bump()?;
        let session = state.current.as_mut().unwrap();
        session.phase = match kind {
            OutputKind::Copy => Phase::Copying,
            OutputKind::Save => Phase::Saving,
        };
        session.output_revision = Some(revision);
        session.error = None;
        Ok(OutputLease {
            image,
            owner,
            operation_revision: revision,
        })
    }

    pub fn finish_output(&self, lease: &OutputLease, error: Option<String>) -> OutputFinish {
        let Ok(mut state) = self.state.lock() else {
            return OutputFinish::Stale;
        };
        if !state.current.as_ref().is_some_and(|s| {
            s.owns(lease.owner) && s.output_revision == Some(lease.operation_revision)
        }) {
            return OutputFinish::Stale;
        }
        if state.bump().is_err() {
            return OutputFinish::Stale;
        }
        let session = state.current.as_mut().unwrap();
        if session.close_pending {
            state.current = None;
            OutputFinish::Closed
        } else {
            session.phase = Phase::Preview;
            session.output_revision = None;
            session.error = error;
            OutputFinish::Preview
        }
    }

    /// Returns whether native window destruction can happen now.
    pub fn close(&self, owner: Owner) -> Result<bool, ServiceError> {
        let mut state = self.state.lock().map_err(|_| ServiceError::Unavailable)?;
        let session = state
            .current
            .as_ref()
            .filter(|s| s.owns(owner))
            .ok_or(ServiceError::Expired)?;
        let busy = matches!(session.phase, Phase::Copying | Phase::Saving);
        state.bump()?;
        if busy {
            state.current.as_mut().unwrap().close_pending = true;
            Ok(false)
        } else {
            state.current = None;
            Ok(true)
        }
    }

    pub fn theme_changed(&self) -> Result<(), ServiceError> {
        let mut state = self.state.lock().map_err(|_| ServiceError::Unavailable)?;
        if state.current.is_some() {
            state.bump()?;
        }
        Ok(())
    }

    pub fn shutdown(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.terminal = true;
            state.current = None;
            let _ = state.bump();
        }
    }

    pub fn abort(&self, ticket: CaptureTicket) -> Option<Origin> {
        let mut state = self.state.lock().ok()?;
        let origin = state
            .current
            .as_ref()
            .filter(|s| s.ticket == ticket)?
            .origin;
        state.current = None;
        let _ = state.bump();
        Some(origin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_output_completion_releases_session_without_a_preview() {
        let service = ScreenshotService::default();
        let Begin::Capture(ticket) = service
            .begin(ScreenshotMode::Region, Origin::Launcher)
            .unwrap()
        else {
            panic!()
        };
        assert!(
            !service.finish_selection_hidden(ticket),
            "not selecting yet"
        );
        assert!(service.selecting(ticket));
        assert!(service.finish_selection_hidden(ticket));
        assert!(service.current_ticket().is_none());
        let Begin::Capture(next) = service
            .begin(ScreenshotMode::Region, Origin::Hotkey)
            .unwrap()
        else {
            panic!()
        };
        assert!(
            !service.finish_selection_hidden(ticket),
            "old completion must not close a new session"
        );
        assert_eq!(service.current_ticket(), Some(next));
    }

    #[test]
    fn hidden_retake_completion_releases_previous_image_but_interruption_preserves_it() {
        let service = ScreenshotService::default();
        let owner = ready(&service);
        assert!(service.budget.used() > 0);
        let ticket = service.retake(owner).unwrap();
        assert!(service.selecting(ticket));
        assert!(service.finish_selection_hidden(ticket));
        assert_eq!(service.budget.used(), 0);
        assert!(service.current_ticket().is_none());
        let owner = ready(&service);
        let ticket = service.retake(owner).unwrap();
        assert!(service.selecting(ticket));
        assert_eq!(
            service.finish_capture(ticket, Ok(None)),
            CaptureFinish::Preview
        );
        assert!(service.budget.used() > 0);
    }

    #[test]
    fn failed_preview_creation_allows_new_capture_but_stale_abort_does_not() {
        let service = ScreenshotService::default();
        let Begin::Capture(a) = service
            .begin(ScreenshotMode::Region, Origin::Launcher)
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(service.abort(a), Some(Origin::Launcher));
        let Begin::Capture(b) = service
            .begin(ScreenshotMode::Window, Origin::Hotkey)
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(service.abort(a), None);
        assert_eq!(service.current_ticket(), Some(b));
    }

    fn captured(service: &ScreenshotService) -> Arc<CapturedImage> {
        Arc::new(CapturedImage::from_encoded(1, 1, vec![1, 2, 3], &service.budget).unwrap())
    }

    fn ready(service: &ScreenshotService) -> Owner {
        let Begin::Capture(ticket) = service
            .begin(ScreenshotMode::Region, Origin::Launcher)
            .unwrap()
        else {
            panic!()
        };
        assert!(service.selecting(ticket));
        assert_eq!(
            service.finish_capture(ticket, Ok(Some(captured(service)))),
            CaptureFinish::Preview
        );
        service.bind_window(ticket, 10).unwrap()
    }

    #[test]
    fn repeated_start_and_stale_completion_have_single_owner() {
        let service = ScreenshotService::default();
        let Begin::Capture(a) = service
            .begin(ScreenshotMode::Region, Origin::Launcher)
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(
            service
                .begin(ScreenshotMode::Window, Origin::Hotkey)
                .unwrap(),
            Begin::Busy
        );
        assert_eq!(
            service.finish_capture(a, Ok(None)),
            CaptureFinish::Cancelled(Origin::Launcher)
        );
        let Begin::Capture(b) = service
            .begin(ScreenshotMode::Window, Origin::Hotkey)
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(
            service.finish_capture(a, Ok(Some(captured(&service)))),
            CaptureFinish::Stale
        );
        assert_eq!(service.current_ticket(), Some(b));
    }

    #[test]
    fn theme_change_advances_snapshot_without_invalidating_output_owner() {
        let service = ScreenshotService::default();
        let owner = ready(&service);
        let before = service.snapshot(owner.window_instance).unwrap();
        let output = service.begin_output(owner, OutputKind::Copy).unwrap();
        service.theme_changed().unwrap();
        let after = service.snapshot(owner.window_instance).unwrap();
        assert!(after.revision.parse::<u64>().unwrap() > before.revision.parse::<u64>().unwrap());
        assert_eq!(after.generation, before.generation);
        assert_eq!(service.finish_output(&output, None), OutputFinish::Preview);
    }

    #[test]
    fn cancelled_retake_retains_previous_pixels_and_changes_generation() {
        let service = ScreenshotService::default();
        let owner = ready(&service);
        let before = service.image(owner).unwrap();
        let ticket = service.retake(owner).unwrap();
        assert!(service.image(owner).is_err());
        assert_eq!(
            service.finish_capture(ticket, Ok(None)),
            CaptureFinish::Preview
        );
        let current = service.owner(10).unwrap();
        assert!(Arc::ptr_eq(&before, &service.image(current).unwrap()));
        assert_ne!(owner.generation, current.generation);
    }

    #[test]
    fn output_pins_pixels_and_defers_close_until_completion() {
        let service = ScreenshotService::default();
        let owner = ready(&service);
        let lease = service.begin_output(owner, OutputKind::Save).unwrap();
        assert!(!service.close(owner).unwrap());
        assert!(service.retake(owner).is_err());
        assert!(service.begin_output(owner, OutputKind::Copy).is_err());
        assert_eq!(service.finish_output(&lease, None), OutputFinish::Closed);
        assert_eq!(lease.image.png, vec![1, 2, 3]);
        assert!(service.current_ticket().is_none());
        assert_eq!(service.finish_output(&lease, None), OutputFinish::Stale);
    }

    #[test]
    fn window_instance_and_session_expiry_reject_old_requests() {
        let service = ScreenshotService::default();
        let owner = ready(&service);
        assert!(service
            .image(Owner {
                window_instance: 9,
                ..owner
            })
            .is_err());
        service.close(owner).unwrap();
        let new_owner = ready(&service);
        assert_ne!(owner.session_id, new_owner.session_id);
        assert!(service.close(owner).is_err());
        assert_eq!(service.owner(10).unwrap(), new_owner);
    }

    #[test]
    fn shutdown_rejects_new_sessions_and_preserves_inflight_output_ownership() {
        let service = ScreenshotService::default();
        let owner = ready(&service);
        let lease = service.begin_output(owner, OutputKind::Copy).unwrap();
        service.shutdown();
        assert!(service
            .begin(ScreenshotMode::Region, Origin::Hotkey)
            .is_err());
        assert_eq!(service.finish_output(&lease, None), OutputFinish::Stale);
        assert_eq!(service.budget.used(), 3);
        drop(lease);
        assert_eq!(service.budget.used(), 0);
    }
}
