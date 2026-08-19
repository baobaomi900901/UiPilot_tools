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
const FLASH_DURATION: Duration = Duration::from_secs(6);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum TrayVisual {
    #[default]
    Normal,
    Reminder,
}

#[derive(Debug, Default)]
struct TrayFlashState {
    active: bool,
    visual: TrayVisual,
    next_toggle: Duration,
    deadline: Duration,
}

impl TrayFlashState {
    fn restart(&mut self, now: Duration) -> Option<TrayVisual> {
        self.active = true;
        self.next_toggle = now + FLASH_INTERVAL;
        self.deadline = now + FLASH_DURATION;
        self.set_visual(TrayVisual::Reminder)
    }

    fn advance(&mut self, now: Duration) -> Option<TrayVisual> {
        if !self.active {
            return None;
        }
        if now >= self.deadline {
            return self.stop();
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
            TrayVisual::Normal => TrayVisual::Reminder,
            TrayVisual::Reminder => TrayVisual::Normal,
        };
        self.set_visual(visual)
    }

    fn adapter_failed(&mut self) -> Option<TrayVisual> {
        self.stop()
    }

    fn shutdown(&mut self) -> Option<TrayVisual> {
        self.stop()
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

enum TrayCommand {
    Restart,
    Shutdown,
}

struct TrayFlashController<P: TrayIconPort> {
    sender: Mutex<Option<Sender<TrayCommand>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    _port: std::marker::PhantomData<P>,
}

impl<P: TrayIconPort> TrayFlashController<P> {
    fn new(port: P) -> Result<Self, NativeEffectError> {
        let (sender, receiver) = mpsc::channel();
        let port = Arc::new(port);
        let worker = thread::Builder::new()
            .name("uipilot-tray-reminder".into())
            .spawn(move || run_worker(port, receiver))
            .map_err(|_| NativeEffectError)?;
        Ok(Self {
            sender: Mutex::new(Some(sender)),
            worker: Mutex::new(Some(worker)),
            _port: std::marker::PhantomData,
        })
    }

    fn restart(&self) -> Result<(), NativeEffectError> {
        self.sender
            .lock()
            .expect("tray sender lock poisoned")
            .as_ref()
            .ok_or(NativeEffectError)?
            .send(TrayCommand::Restart)
            .map_err(|_| NativeEffectError)
    }

    fn shutdown(&self) {
        if let Some(sender) = self
            .sender
            .lock()
            .expect("tray sender lock poisoned")
            .take()
        {
            let _ = sender.send(TrayCommand::Shutdown);
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

fn run_worker<P: TrayIconPort>(port: Arc<P>, receiver: Receiver<TrayCommand>) {
    let started = Instant::now();
    let mut state = TrayFlashState::default();
    loop {
        if state.active {
            match receiver.recv_timeout(FLASH_INTERVAL) {
                Ok(TrayCommand::Restart) => {
                    let action = state.restart(started.elapsed());
                    apply_or_stop(port.as_ref(), &mut state, action);
                }
                Ok(TrayCommand::Shutdown) => {
                    let action = state.shutdown();
                    apply_best_effort(port.as_ref(), action);
                    break;
                }
                Err(RecvTimeoutError::Timeout) => {
                    let action = state.advance(started.elapsed());
                    apply_or_stop(port.as_ref(), &mut state, action);
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let action = state.shutdown();
                    apply_best_effort(port.as_ref(), action);
                    break;
                }
            }
        } else {
            match receiver.recv() {
                Ok(TrayCommand::Restart) => {
                    let action = state.restart(started.elapsed());
                    apply_or_stop(port.as_ref(), &mut state, action);
                }
                Ok(TrayCommand::Shutdown) | Err(_) => {
                    let action = state.shutdown();
                    apply_best_effort(port.as_ref(), action);
                    break;
                }
            }
        }
    }
}

fn apply_or_stop<P: TrayIconPort>(
    port: &P,
    state: &mut TrayFlashState,
    action: Option<TrayVisual>,
) {
    if action.is_some_and(|visual| port.set_visual(visual).is_err()) {
        eprintln!("[message-center] tray icon update failed");
        let cleanup = state.adapter_failed();
        apply_best_effort(port, cleanup);
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
    reminder: Image<'static>,
}

impl<R: Runtime> TrayIconPort for TauriTrayPort<R> {
    fn set_visual(&self, visual: TrayVisual) -> Result<(), NativeEffectError> {
        let icon = match visual {
            TrayVisual::Normal => self.normal.clone(),
            TrayVisual::Reminder => self.reminder.clone(),
        };
        self.tray
            .set_icon(Some(icon))
            .map_err(|_| NativeEffectError)
    }
}

pub(crate) struct TauriTrayReminder<R: Runtime> {
    controller: TrayFlashController<TauriTrayPort<R>>,
}

impl<R: Runtime> TauriTrayReminder<R> {
    pub(crate) fn new(
        tray: TrayIcon<R>,
        normal: Image<'static>,
        reminder: Image<'static>,
    ) -> Result<Self, NativeEffectError> {
        TrayFlashController::new(TauriTrayPort {
            tray,
            normal,
            reminder,
        })
        .map(|controller| Self { controller })
    }
}

impl<R: Runtime> MessageTray for TauriTrayReminder<R> {
    fn restart(&self) -> Result<(), NativeEffectError> {
        self.controller.restart()
    }

    fn shutdown(&self) {
        self.controller.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{TrayFlashState, TrayVisual};

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn flash_uses_500ms_cadence_and_restores_normal_at_six_seconds() {
        let mut state = TrayFlashState::default();

        assert_eq!(state.restart(ms(0)), Some(TrayVisual::Reminder));
        assert_eq!(state.advance(ms(499)), None);
        assert_eq!(state.advance(ms(500)), Some(TrayVisual::Normal));
        assert_eq!(state.advance(ms(1_000)), Some(TrayVisual::Reminder));
        assert_eq!(state.advance(ms(6_000)), Some(TrayVisual::Normal));
        assert_eq!(state.advance(ms(6_500)), None);
    }

    #[test]
    fn a_new_message_replaces_the_single_deadline() {
        let mut state = TrayFlashState::default();

        assert_eq!(state.restart(ms(0)), Some(TrayVisual::Reminder));
        assert_eq!(state.advance(ms(500)), Some(TrayVisual::Normal));
        assert_eq!(state.restart(ms(1_000)), Some(TrayVisual::Reminder));
        assert_ne!(state.advance(ms(6_000)), Some(TrayVisual::Normal));
        assert_eq!(state.advance(ms(7_000)), Some(TrayVisual::Normal));
        assert_eq!(state.advance(ms(7_500)), None);
    }

    #[test]
    fn adapter_failure_and_shutdown_restore_normal_idempotently() {
        let mut state = TrayFlashState::default();
        state.restart(ms(0));

        assert_eq!(state.adapter_failed(), Some(TrayVisual::Normal));
        assert_eq!(state.adapter_failed(), None);
        assert_eq!(state.restart(ms(1_000)), Some(TrayVisual::Reminder));
        assert_eq!(state.shutdown(), Some(TrayVisual::Normal));
        assert_eq!(state.shutdown(), None);
    }
}
