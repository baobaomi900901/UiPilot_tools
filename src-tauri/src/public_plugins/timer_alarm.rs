use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use super::timers::AudioTicket;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TimerAlarmError;

pub(crate) trait TimerAlarm: Send + Sync {
    fn play(&self, ticket: &AudioTicket) -> Result<bool, TimerAlarmError>;
    fn stop(&self, ticket: &AudioTicket) -> Result<(), TimerAlarmError>;
    fn shutdown(&self);
}

trait SoundBackend: Send + Sync {
    fn play(&self, path: &Path) -> Result<(), ()>;
    fn stop(&self) -> Result<(), ()>;
}

#[derive(Default)]
struct AlarmState {
    current: Option<AudioTicket>,
    cancelled: BTreeSet<AudioTicket>,
}

struct TimerAlarmController<B: SoundBackend> {
    path: PathBuf,
    backend: Arc<B>,
    state: Mutex<AlarmState>,
}

impl<B: SoundBackend> TimerAlarmController<B> {
    fn new(path: PathBuf, backend: Arc<B>) -> Self {
        Self {
            path,
            backend,
            state: Mutex::new(AlarmState::default()),
        }
    }
}

impl<B: SoundBackend> TimerAlarm for TimerAlarmController<B> {
    fn play(&self, ticket: &AudioTicket) -> Result<bool, TimerAlarmError> {
        let mut state = self.state.lock().map_err(|_| TimerAlarmError)?;
        if state.cancelled.remove(ticket) {
            return Ok(false);
        }
        self.backend.play(&self.path).map_err(|_| TimerAlarmError)?;
        state.current = Some(ticket.clone());
        Ok(true)
    }

    fn stop(&self, ticket: &AudioTicket) -> Result<(), TimerAlarmError> {
        let mut state = self.state.lock().map_err(|_| TimerAlarmError)?;
        state.cancelled.insert(ticket.clone());
        if state.current.as_ref() == Some(ticket) {
            self.backend.stop().map_err(|_| TimerAlarmError)?;
            state.current = None;
        }
        Ok(())
    }

    fn shutdown(&self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.current.take().is_some() {
            let _ = self.backend.stop();
        }
        state.cancelled.clear();
    }
}

#[cfg(windows)]
struct WindowsSoundBackend;

#[cfg(windows)]
impl SoundBackend for WindowsSoundBackend {
    fn play(&self, path: &Path) -> Result<(), ()> {
        use std::{iter, os::windows::ffi::OsStrExt};
        use windows::{
            core::PCWSTR,
            Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT},
        };

        let wide = path
            .as_os_str()
            .encode_wide()
            .chain(iter::once(0))
            .collect::<Vec<_>>();
        unsafe {
            PlaySoundW(
                PCWSTR(wide.as_ptr()),
                None,
                SND_FILENAME | SND_ASYNC | SND_NODEFAULT,
            )
        }
        .as_bool()
        .then_some(())
        .ok_or(())
    }

    fn stop(&self) -> Result<(), ()> {
        use windows::{
            core::PCWSTR,
            Win32::Media::Audio::{PlaySoundW, SND_FLAGS},
        };
        unsafe { PlaySoundW(PCWSTR::null(), None, SND_FLAGS(0)) }
            .as_bool()
            .then_some(())
            .ok_or(())
    }
}

#[cfg(windows)]
pub(crate) fn windows_alarm(path: PathBuf) -> Arc<dyn TimerAlarm> {
    Arc::new(TimerAlarmController::new(
        path,
        Arc::new(WindowsSoundBackend),
    ))
}

#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        sync::{Arc, Mutex},
    };

    use super::{SoundBackend, TimerAlarm, TimerAlarmController};
    use crate::public_plugins::timers::{AudioTicket, TimerKey};

    #[derive(Default)]
    struct FakeBackend(Mutex<Vec<&'static str>>);

    impl SoundBackend for FakeBackend {
        fn play(&self, _path: &Path) -> Result<(), ()> {
            self.0.lock().unwrap().push("play");
            Ok(())
        }

        fn stop(&self) -> Result<(), ()> {
            self.0.lock().unwrap().push("stop");
            Ok(())
        }
    }

    fn ticket(audio_id: u64) -> AudioTicket {
        AudioTicket {
            key: TimerKey::new("com.example.timer", 1).unwrap(),
            round_id: 1,
            audio_id,
            fired_revision: audio_id,
        }
    }

    #[test]
    fn cancellation_before_play_absorbs_late_audio_start() {
        let backend = Arc::new(FakeBackend::default());
        let alarm = TimerAlarmController::new("timer.wav".into(), backend.clone());
        let ticket = ticket(1);

        alarm.stop(&ticket).unwrap();
        assert!(!alarm.play(&ticket).unwrap());
        assert!(backend.0.lock().unwrap().is_empty());
    }

    #[test]
    fn cancellation_after_play_stops_only_the_current_ticket() {
        let backend = Arc::new(FakeBackend::default());
        let alarm = TimerAlarmController::new("timer.wav".into(), backend.clone());
        let first = ticket(1);
        let second = ticket(2);

        assert!(alarm.play(&first).unwrap());
        alarm.stop(&second).unwrap();
        alarm.stop(&first).unwrap();
        assert_eq!(*backend.0.lock().unwrap(), ["play", "stop"]);
    }
}
