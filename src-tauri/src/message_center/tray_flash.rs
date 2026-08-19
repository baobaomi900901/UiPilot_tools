use std::{
    sync::{
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use tauri::{image::Image, tray::TrayIcon, Runtime};

use super::{MessageTray, NativeEffectError};

const FLASH_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum TrayVisual {
    #[default]
    Normal,
    Transparent,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum TrayAttentionMode {
    #[default]
    Running,
    Degraded,
    Terminal,
}

#[derive(Debug, Default)]
struct TrayAttentionState {
    main_focused: bool,
    active: bool,
    visual: TrayVisual,
    next_toggle: Duration,
    mode: TrayAttentionMode,
}

impl TrayAttentionState {
    fn message_arrived(&mut self, now: Duration) -> Option<TrayVisual> {
        if self.mode != TrayAttentionMode::Running || self.main_focused || self.active {
            return None;
        }
        self.active = true;
        self.next_toggle = now + FLASH_INTERVAL;
        self.set_visual(TrayVisual::Transparent)
    }

    fn main_focus_changed(&mut self, focused: bool) -> Option<TrayVisual> {
        self.main_focused = focused;
        if !focused || self.mode == TrayAttentionMode::Terminal {
            return None;
        }
        if self.mode == TrayAttentionMode::Degraded {
            self.active = false;
            self.visual = TrayVisual::Normal;
            return Some(TrayVisual::Normal);
        }
        self.stop()
    }

    fn advance(&mut self, now: Duration) -> Option<TrayVisual> {
        if self.mode != TrayAttentionMode::Running || !self.active {
            return None;
        }
        if now < self.next_toggle {
            return None;
        }

        let elapsed = now - self.next_toggle;
        let intervals = elapsed.as_millis() / FLASH_INTERVAL.as_millis() + 1;
        self.next_toggle += FLASH_INTERVAL * u32::try_from(intervals).unwrap_or(u32::MAX);
        if intervals.is_multiple_of(2) {
            return None;
        }
        let visual = match self.visual {
            TrayVisual::Normal => TrayVisual::Transparent,
            TrayVisual::Transparent => TrayVisual::Normal,
        };
        self.set_visual(visual)
    }

    fn adapter_failed(&mut self) -> Option<TrayVisual> {
        if self.mode != TrayAttentionMode::Running {
            return None;
        }
        self.mode = TrayAttentionMode::Degraded;
        self.active = false;
        self.visual = TrayVisual::Normal;
        Some(TrayVisual::Normal)
    }

    fn shutdown(&mut self) -> Option<TrayVisual> {
        if self.mode == TrayAttentionMode::Terminal {
            return None;
        }
        self.mode = TrayAttentionMode::Terminal;
        self.active = false;
        self.visual = TrayVisual::Normal;
        Some(TrayVisual::Normal)
    }

    fn stop(&mut self) -> Option<TrayVisual> {
        self.active = false;
        self.set_visual(TrayVisual::Normal)
    }

    fn set_visual(&mut self, visual: TrayVisual) -> Option<TrayVisual> {
        if self.visual == visual {
            return None;
        }
        self.visual = visual;
        Some(visual)
    }
}

trait TrayIconPort: Send + Sync + 'static {
    fn set_visual(&self, visual: TrayVisual) -> Result<(), NativeEffectError>;
}

enum TrayAttentionEvent {
    MessageArrived,
    MainFocusChanged(bool),
    Shutdown,
}

struct TrayFlashController<P: TrayIconPort> {
    sender: Mutex<Option<Sender<TrayAttentionEvent>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    _port: std::marker::PhantomData<P>,
}

impl<P: TrayIconPort> TrayFlashController<P> {
    fn new(port: P) -> Self {
        Self::new_with_spawner(port, |port, receiver| {
            thread::Builder::new()
                .name("uipilot-tray-attention".into())
                .spawn(move || run_worker(port, receiver))
        })
    }

    fn new_with_spawner<F>(port: P, spawn: F) -> Self
    where
        F: FnOnce(Arc<P>, Receiver<TrayAttentionEvent>) -> std::io::Result<JoinHandle<()>>,
    {
        let (sender, receiver) = mpsc::channel();
        let port = Arc::new(port);
        let (sender, worker) = match spawn(port, receiver) {
            Ok(worker) => (Some(sender), Some(worker)),
            Err(_) => {
                eprintln!("[message-center] tray attention controller unavailable");
                (None, None)
            }
        };
        Self {
            sender: Mutex::new(sender),
            worker: Mutex::new(worker),
            _port: std::marker::PhantomData,
        }
    }

    fn message_arrived(&self) -> Result<(), NativeEffectError> {
        self.send(TrayAttentionEvent::MessageArrived)
    }

    fn main_focus_changed(&self, focused: bool) -> Result<(), NativeEffectError> {
        self.send(TrayAttentionEvent::MainFocusChanged(focused))
    }

    fn send(&self, event: TrayAttentionEvent) -> Result<(), NativeEffectError> {
        let mut sender = self.sender.lock().expect("tray sender lock poisoned");
        let Some(active) = sender.as_ref() else {
            return Ok(());
        };
        if active.send(event).is_err() {
            sender.take();
            return Err(NativeEffectError);
        }
        Ok(())
    }

    fn shutdown(&self) {
        if let Some(sender) = self
            .sender
            .lock()
            .expect("tray sender lock poisoned")
            .take()
        {
            let _ = sender.send(TrayAttentionEvent::Shutdown);
        }
        if let Some(worker) = self
            .worker
            .lock()
            .expect("tray worker lock poisoned")
            .take()
        {
            let _ = worker.join();
        }
    }
}

fn run_worker<P: TrayIconPort>(port: Arc<P>, receiver: Receiver<TrayAttentionEvent>) {
    let started = Instant::now();
    let mut state = TrayAttentionState::default();
    loop {
        if state.active {
            let wait = state.next_toggle.saturating_sub(started.elapsed());
            match receiver.recv_timeout(wait) {
                Ok(TrayAttentionEvent::MessageArrived) => {
                    let action = state.message_arrived(started.elapsed());
                    apply_transition(port.as_ref(), &mut state, action);
                }
                Ok(TrayAttentionEvent::MainFocusChanged(focused)) => {
                    let action = state.main_focus_changed(focused);
                    apply_transition(port.as_ref(), &mut state, action);
                }
                Ok(TrayAttentionEvent::Shutdown) => {
                    let action = state.shutdown();
                    apply_best_effort(port.as_ref(), action);
                    break;
                }
                Err(RecvTimeoutError::Timeout) => {
                    let action = state.advance(started.elapsed());
                    apply_transition(port.as_ref(), &mut state, action);
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let action = state.shutdown();
                    apply_best_effort(port.as_ref(), action);
                    break;
                }
            }
        } else {
            match receiver.recv() {
                Ok(TrayAttentionEvent::MessageArrived) => {
                    let action = state.message_arrived(started.elapsed());
                    apply_transition(port.as_ref(), &mut state, action);
                }
                Ok(TrayAttentionEvent::MainFocusChanged(focused)) => {
                    let action = state.main_focus_changed(focused);
                    apply_transition(port.as_ref(), &mut state, action);
                }
                Ok(TrayAttentionEvent::Shutdown) | Err(_) => {
                    let action = state.shutdown();
                    apply_best_effort(port.as_ref(), action);
                    break;
                }
            }
        }
    }
}

fn apply_transition<P: TrayIconPort>(
    port: &P,
    state: &mut TrayAttentionState,
    action: Option<TrayVisual>,
) {
    if action.is_some_and(|visual| port.set_visual(visual).is_err()) {
        eprintln!("[message-center] tray icon update failed");
        if state.mode == TrayAttentionMode::Running {
            let cleanup = state.adapter_failed();
            apply_best_effort(port, cleanup);
        }
    }
}

fn apply_best_effort<P: TrayIconPort>(port: &P, action: Option<TrayVisual>) {
    if let Some(visual) = action {
        let _ = port.set_visual(visual);
    }
}

struct TauriTrayPort<R: Runtime> {
    tray: TrayIcon<R>,
    normal: Image<'static>,
}

impl<R: Runtime> TrayIconPort for TauriTrayPort<R> {
    fn set_visual(&self, visual: TrayVisual) -> Result<(), NativeEffectError> {
        let icon = match visual {
            TrayVisual::Normal => Some(self.normal.clone()),
            TrayVisual::Transparent => None,
        };
        self.tray.set_icon(icon).map_err(|_| NativeEffectError)
    }
}

pub(crate) struct TauriTrayReminder<R: Runtime> {
    controller: TrayFlashController<TauriTrayPort<R>>,
    _tray: TrayIcon<R>,
}

impl<R: Runtime> TauriTrayReminder<R> {
    pub(crate) fn new(tray: TrayIcon<R>, normal: Image<'static>) -> Self {
        let controller = TrayFlashController::new(TauriTrayPort {
            tray: tray.clone(),
            normal,
        });
        Self {
            controller,
            _tray: tray,
        }
    }
}

impl<R: Runtime> MessageTray for TauriTrayReminder<R> {
    fn message_arrived(&self) -> Result<(), NativeEffectError> {
        self.controller.message_arrived()
    }

    fn main_focus_changed(&self, focused: bool) -> Result<(), NativeEffectError> {
        self.controller.main_focus_changed(focused)
    }

    fn shutdown(&self) {
        self.controller.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use std::{io, sync::Mutex, time::Duration};

    use super::{
        apply_transition, TrayAttentionMode, TrayAttentionState, TrayFlashController, TrayIconPort,
        TrayVisual,
    };
    use crate::message_center::NativeEffectError;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn unfocused_message_flashes_indefinitely_at_500ms_cadence() {
        let mut state = TrayAttentionState::default();

        assert_eq!(state.message_arrived(ms(0)), Some(TrayVisual::Transparent));
        assert_eq!(state.advance(ms(499)), None);
        assert_eq!(state.advance(ms(500)), Some(TrayVisual::Normal));
        assert_eq!(state.advance(ms(1_000)), Some(TrayVisual::Transparent));
        assert_eq!(state.advance(ms(60_500)), Some(TrayVisual::Normal));
        assert_eq!(state.mode, TrayAttentionMode::Running);
        assert!(state.active);
    }

    #[test]
    fn current_focus_suppresses_messages_and_blur_does_not_start_attention() {
        let mut state = TrayAttentionState::default();

        assert_eq!(state.main_focus_changed(true), None);
        assert_eq!(state.message_arrived(ms(0)), None);
        assert_eq!(state.main_focus_changed(false), None);
        assert_eq!(state.advance(ms(500)), None);
        assert_eq!(
            state.message_arrived(ms(600)),
            Some(TrayVisual::Transparent)
        );
    }

    #[test]
    fn focus_stops_attention_while_repeated_messages_keep_one_cadence() {
        let mut state = TrayAttentionState::default();

        assert_eq!(state.message_arrived(ms(0)), Some(TrayVisual::Transparent));
        assert_eq!(state.message_arrived(ms(200)), None);
        assert_eq!(state.advance(ms(500)), Some(TrayVisual::Normal));
        assert_eq!(state.main_focus_changed(true), None);
        assert!(!state.active);
        assert_eq!(state.main_focus_changed(false), None);
        assert_eq!(state.advance(ms(1_000)), None);
    }

    #[test]
    fn adapter_failure_and_shutdown_are_absorbing_states() {
        let mut state = TrayAttentionState::default();
        state.message_arrived(ms(0));

        assert_eq!(state.adapter_failed(), Some(TrayVisual::Normal));
        assert_eq!(state.mode, TrayAttentionMode::Degraded);
        assert_eq!(state.message_arrived(ms(1_000)), None);
        assert_eq!(state.main_focus_changed(true), Some(TrayVisual::Normal));
        assert_eq!(state.shutdown(), Some(TrayVisual::Normal));
        assert_eq!(state.mode, TrayAttentionMode::Terminal);
        assert_eq!(state.message_arrived(ms(2_000)), None);
        assert_eq!(state.shutdown(), None);
    }

    struct FailingPort(Mutex<Vec<TrayVisual>>);

    impl TrayIconPort for FailingPort {
        fn set_visual(&self, visual: TrayVisual) -> Result<(), NativeEffectError> {
            self.0.lock().unwrap().push(visual);
            Err(NativeEffectError)
        }
    }

    #[test]
    fn visual_failure_attempts_normal_once_and_degrades() {
        let port = FailingPort(Mutex::new(Vec::new()));
        let mut state = TrayAttentionState::default();
        let action = state.message_arrived(ms(0));

        apply_transition(&port, &mut state, action);

        assert_eq!(
            *port.0.lock().unwrap(),
            [TrayVisual::Transparent, TrayVisual::Normal]
        );
        assert_eq!(state.mode, TrayAttentionMode::Degraded);
        assert_eq!(state.message_arrived(ms(500)), None);
    }

    #[test]
    fn worker_construction_failure_installs_an_idempotent_noop_controller() {
        let controller = TrayFlashController::new_with_spawner(
            FailingPort(Mutex::new(Vec::new())),
            |_port, _receiver| Err(io::Error::other("worker unavailable")),
        );

        assert_eq!(controller.message_arrived(), Ok(()));
        assert_eq!(controller.main_focus_changed(true), Ok(()));
        controller.shutdown();
        controller.shutdown();
    }
}
