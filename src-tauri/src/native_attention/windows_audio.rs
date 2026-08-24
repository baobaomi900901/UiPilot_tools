use std::{
    mem::{self, ManuallyDrop},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};

use windows::{
    core::PCWSTR,
    Win32::Media::Audio::{
        PlaySoundW, SND_ASYNC, SND_FILENAME, SND_FLAGS, SND_LOOP, SND_MEMORY, SND_NODEFAULT,
    },
};

use crate::message_center::NativeEffectError;

use super::{AlarmPlaybackKey, AttentionAudioPort};

trait SoundBackend: Send + Sync {
    fn play_file(&mut self, path: &Path) -> Result<(), NativeEffectError>;
    fn play_memory(&mut self, bytes: &[u8]) -> Result<(), NativeEffectError>;
    fn stop(&mut self) -> Result<(), NativeEffectError>;
}

struct WindowsSoundBackend;

impl SoundBackend for WindowsSoundBackend {
    fn play_file(&mut self, path: &Path) -> Result<(), NativeEffectError> {
        let wide = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
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
        .ok_or(NativeEffectError)
    }

    fn play_memory(&mut self, bytes: &[u8]) -> Result<(), NativeEffectError> {
        unsafe {
            PlaySoundW(
                PCWSTR(bytes.as_ptr().cast()),
                None,
                SND_MEMORY | SND_ASYNC | SND_LOOP | SND_NODEFAULT,
            )
        }
        .as_bool()
        .then_some(())
        .ok_or(NativeEffectError)
    }

    fn stop(&mut self) -> Result<(), NativeEffectError> {
        unsafe { PlaySoundW(PCWSTR::null(), None, SND_FLAGS(0)) }
            .as_bool()
            .then_some(())
            .ok_or(NativeEffectError)
    }
}

struct PlayingMemory {
    key: AlarmPlaybackKey,
    bytes: Arc<[u8]>,
}

struct AdapterState<B> {
    backend: B,
    playing: Option<PlayingMemory>,
    ordinary_playing: bool,
    terminal: bool,
}

struct WindowsAudioPort<B: SoundBackend> {
    message_path: PathBuf,
    state: Mutex<AdapterState<B>>,
}

impl<B: SoundBackend> AttentionAudioPort for WindowsAudioPort<B> {
    fn play_ordinary(&self) -> Result<(), NativeEffectError> {
        let mut state = self.lock_available()?;
        if state.playing.is_some() {
            return Err(NativeEffectError);
        }
        state.backend.play_file(&self.message_path)?;
        state.ordinary_playing = true;
        Ok(())
    }

    fn stop_ordinary(&self) -> Result<(), NativeEffectError> {
        let mut state = self.lock_available()?;
        if state.playing.is_some() || !state.ordinary_playing {
            return Ok(());
        }
        state.backend.stop()?;
        state.ordinary_playing = false;
        Ok(())
    }

    fn start_timer_loop(
        &self,
        key: AlarmPlaybackKey,
        bytes: Arc<[u8]>,
    ) -> Result<(), NativeEffectError> {
        let mut state = self.lock_available()?;
        if state.playing.is_some() {
            return Err(NativeEffectError);
        }
        if state.ordinary_playing {
            state.backend.stop()?;
            state.ordinary_playing = false;
        }
        state.playing = Some(PlayingMemory { key, bytes });
        let accepted = {
            let AdapterState {
                backend, playing, ..
            } = &mut *state;
            let bytes = &playing.as_ref().expect("playing memory missing").bytes;
            backend.play_memory(bytes)
        };
        if accepted.is_err() {
            state.playing = None;
        }
        accepted
    }

    fn stop_timer_loop(&self, key: AlarmPlaybackKey) -> Result<(), NativeEffectError> {
        let mut state = self.lock_available()?;
        if state
            .playing
            .as_ref()
            .is_none_or(|playing| playing.key != key)
        {
            return Ok(());
        }
        match state.backend.stop() {
            Ok(()) => {
                state.playing = None;
                Ok(())
            }
            Err(error) => {
                if let Some(playing) = state.playing.take() {
                    quarantine(playing.bytes);
                }
                state.terminal = true;
                Err(error)
            }
        }
    }

    fn shutdown(&self) {
        self.shutdown_inner();
    }
}

impl<B: SoundBackend> WindowsAudioPort<B> {
    fn lock_available(&self) -> Result<MutexGuard<'_, AdapterState<B>>, NativeEffectError> {
        match self.state.lock() {
            Ok(state) if !state.terminal => Ok(state),
            Ok(_) => Err(NativeEffectError),
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                state.terminal = true;
                if let Some(playing) = state.playing.take() {
                    quarantine(playing.bytes);
                }
                Err(NativeEffectError)
            }
        }
    }

    fn shutdown_inner(&self) {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                state.terminal = true;
                if let Some(playing) = state.playing.take() {
                    quarantine(playing.bytes);
                }
                return;
            }
        };
        if state.terminal {
            return;
        }
        state.terminal = true;
        if state.playing.is_some() {
            if state.backend.stop().is_ok() {
                state.playing = None;
            } else if let Some(playing) = state.playing.take() {
                quarantine(playing.bytes);
            }
        } else if state.ordinary_playing {
            let _ = state.backend.stop();
            state.ordinary_playing = false;
        }
    }
}

impl<B: SoundBackend> Drop for WindowsAudioPort<B> {
    fn drop(&mut self) {
        self.shutdown_inner();
    }
}

type QuarantinedAudio = Option<ManuallyDrop<Arc<[u8]>>>;

static PROCESS_AUDIO_QUARANTINE: OnceLock<Mutex<QuarantinedAudio>> = OnceLock::new();

fn quarantine(bytes: Arc<[u8]>) {
    let slot = PROCESS_AUDIO_QUARANTINE.get_or_init(|| Mutex::new(None));
    let mut slot = match slot.lock() {
        Ok(slot) => slot,
        Err(poisoned) => poisoned.into_inner(),
    };
    if slot.is_none() {
        *slot = Some(ManuallyDrop::new(bytes));
    } else {
        mem::forget(bytes);
    }
}

pub(crate) fn windows_audio(message_path: PathBuf) -> Arc<dyn AttentionAudioPort> {
    Arc::new(WindowsAudioPort {
        message_path,
        state: Mutex::new(AdapterState {
            backend: WindowsSoundBackend,
            playing: None,
            ordinary_playing: false,
            terminal: false,
        }),
    })
}

#[cfg(test)]
mod tests {
    use std::{
        panic::{self, AssertUnwindSafe},
        path::Path,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Condvar, Mutex,
        },
        thread,
    };

    use sha2::{Digest, Sha256};

    use super::{AdapterState, SoundBackend, WindowsAudioPort};
    use crate::{
        message_center::NativeEffectError,
        native_attention::{AlarmPlaybackKey, AttentionAudioPort},
    };

    #[derive(Default)]
    struct FakeBackend {
        calls: Vec<String>,
        fail_stop: bool,
    }

    #[derive(Default)]
    struct SerialProbe {
        active: AtomicUsize,
        maximum: AtomicUsize,
        entered: Mutex<bool>,
        released: Mutex<bool>,
        changed: Condvar,
    }

    impl SerialProbe {
        fn enter(&self) {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum.fetch_max(active, Ordering::SeqCst);
            *self.entered.lock().unwrap() = true;
            self.changed.notify_all();
            let released = self.released.lock().unwrap();
            drop(self.changed.wait_while(released, |value| !*value).unwrap());
            self.active.fetch_sub(1, Ordering::SeqCst);
        }

        fn wait_entered(&self) {
            let entered = self.entered.lock().unwrap();
            drop(self.changed.wait_while(entered, |value| !*value).unwrap());
        }

        fn release(&self) {
            *self.released.lock().unwrap() = true;
            self.changed.notify_all();
        }
    }

    struct SerialBackend(Arc<SerialProbe>);

    impl SoundBackend for SerialBackend {
        fn play_file(&mut self, _path: &Path) -> Result<(), NativeEffectError> {
            self.0.enter();
            Ok(())
        }

        fn play_memory(&mut self, _bytes: &[u8]) -> Result<(), NativeEffectError> {
            self.0.enter();
            Ok(())
        }

        fn stop(&mut self) -> Result<(), NativeEffectError> {
            Ok(())
        }
    }

    impl SoundBackend for FakeBackend {
        fn play_file(&mut self, path: &Path) -> Result<(), NativeEffectError> {
            self.calls.push(format!("{}:once", path.display()));
            Ok(())
        }

        fn play_memory(&mut self, bytes: &[u8]) -> Result<(), NativeEffectError> {
            self.calls.push(format!("memory:{}:loop", bytes.len()));
            Ok(())
        }

        fn stop(&mut self) -> Result<(), NativeEffectError> {
            self.calls.push("stop".into());
            (!self.fail_stop).then_some(()).ok_or(NativeEffectError)
        }
    }

    #[test]
    fn one_shot_and_memory_loop_share_one_serial_adapter() {
        let port = WindowsAudioPort {
            message_path: "message.wav".into(),
            state: Mutex::new(AdapterState {
                backend: FakeBackend::default(),
                playing: None,
                ordinary_playing: false,
                terminal: false,
            }),
        };
        let bytes: Arc<[u8]> = Arc::from([1_u8, 2, 3, 4]);

        port.play_ordinary().unwrap();
        port.start_timer_loop(AlarmPlaybackKey::new_for_test(7), Arc::clone(&bytes))
            .unwrap();
        port.stop_timer_loop(AlarmPlaybackKey::new_for_test(8))
            .unwrap();
        assert_eq!(Arc::strong_count(&bytes), 2);
        port.stop_timer_loop(AlarmPlaybackKey::new_for_test(7))
            .unwrap();
        assert_eq!(Arc::strong_count(&bytes), 1);

        assert_eq!(
            port.state.lock().unwrap().backend.calls,
            ["message.wav:once", "stop", "memory:4:loop", "stop"]
        );
    }

    #[test]
    fn host_bundles_only_the_approved_ordinary_message_sound() {
        let message = include_bytes!("../../resources/sounds/message-notification.wav");
        assert_eq!(message.len(), 684_044);
        assert_eq!(&message[..4], b"RIFF");
        assert_eq!(&message[8..12], b"WAVE");
        assert_eq!(
            format!("{:X}", Sha256::digest(message)),
            "B29D9BF3E4942C5372159A641203A20F124E4D58DCBEE8B272957423701D7766"
        );
        let config = include_str!("../../tauri.conf.json");
        assert!(config.contains("resources/sounds/message-notification.wav"));
        assert!(!config.contains("resources/sounds/attention-alarm.wav"));
        assert!(!config.contains("resources/sounds/timer-complete.wav"));
        let bootstrap = include_str!("../lib.rs");
        assert!(!bootstrap.contains("\"resources/sounds/attention-alarm.wav\""));
        assert!(bootstrap.contains("\"resources/sounds/message-notification.wav\""));
    }

    #[test]
    fn stop_failure_quarantines_memory_and_makes_the_adapter_terminal() {
        let port = WindowsAudioPort {
            message_path: "message.wav".into(),
            state: Mutex::new(AdapterState {
                backend: FakeBackend::default(),
                playing: None,
                ordinary_playing: false,
                terminal: false,
            }),
        };
        let key = AlarmPlaybackKey::new_for_test(11);
        let bytes: Arc<[u8]> = Arc::from([9_u8; 32]);
        let weak = Arc::downgrade(&bytes);
        port.start_timer_loop(key, Arc::clone(&bytes)).unwrap();
        drop(bytes);
        port.state.lock().unwrap().backend.fail_stop = true;

        assert!(port.stop_timer_loop(key).is_err());
        assert!(weak.upgrade().is_some());
        assert!(port.play_ordinary().is_err());
        assert!(port
            .start_timer_loop(AlarmPlaybackKey::new_for_test(12), Arc::from([1_u8]))
            .is_err());
    }

    #[test]
    fn all_backend_calls_are_serialized_by_the_adapter() {
        let probe = Arc::new(SerialProbe::default());
        let port = Arc::new(WindowsAudioPort {
            message_path: "message.wav".into(),
            state: Mutex::new(AdapterState {
                backend: SerialBackend(Arc::clone(&probe)),
                playing: None,
                ordinary_playing: false,
                terminal: false,
            }),
        });
        let starter = {
            let port = Arc::clone(&port);
            thread::spawn(move || {
                port.start_timer_loop(AlarmPlaybackKey::new_for_test(21), Arc::from([1_u8; 8]))
            })
        };
        probe.wait_entered();
        let ordinary = {
            let port = Arc::clone(&port);
            thread::spawn(move || port.play_ordinary())
        };
        probe.release();

        assert!(starter.join().unwrap().is_ok());
        assert!(ordinary.join().unwrap().is_err());
        assert_eq!(probe.maximum.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn poisoned_state_quarantines_playing_memory_before_failing_closed() {
        let port = WindowsAudioPort {
            message_path: "message.wav".into(),
            state: Mutex::new(AdapterState {
                backend: FakeBackend::default(),
                playing: None,
                ordinary_playing: false,
                terminal: false,
            }),
        };
        let bytes: Arc<[u8]> = Arc::from([5_u8; 16]);
        let weak = Arc::downgrade(&bytes);
        port.start_timer_loop(AlarmPlaybackKey::new_for_test(31), Arc::clone(&bytes))
            .unwrap();
        drop(bytes);
        let _ = panic::catch_unwind(AssertUnwindSafe(|| {
            let _state = port.state.lock().unwrap();
            panic!("poison adapter state");
        }));

        assert!(port.play_ordinary().is_err());
        assert!(weak.upgrade().is_some());
    }

    #[test]
    fn adapter_drop_stops_before_releasing_playing_memory() {
        let port = WindowsAudioPort {
            message_path: "message.wav".into(),
            state: Mutex::new(AdapterState {
                backend: FakeBackend::default(),
                playing: None,
                ordinary_playing: false,
                terminal: false,
            }),
        };
        let bytes: Arc<[u8]> = Arc::from([3_u8; 16]);
        let weak = Arc::downgrade(&bytes);
        port.start_timer_loop(AlarmPlaybackKey::new_for_test(41), Arc::clone(&bytes))
            .unwrap();
        drop(bytes);

        drop(port);

        assert!(weak.upgrade().is_none());
    }
}
