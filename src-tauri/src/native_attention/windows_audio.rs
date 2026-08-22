use std::{
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::Arc,
};

use windows::{
    core::PCWSTR,
    Win32::Media::Audio::{
        PlaySoundW, SND_ASYNC, SND_FILENAME, SND_FLAGS, SND_LOOP, SND_NODEFAULT,
    },
};

use crate::{message_center::NativeEffectError, public_plugins::AudioTicket};

use super::AttentionAudioPort;

trait SoundBackend: Send + Sync {
    fn play(&self, path: &Path, looped: bool) -> Result<(), NativeEffectError>;
    fn stop(&self) -> Result<(), NativeEffectError>;
}

struct WindowsSoundBackend;

impl SoundBackend for WindowsSoundBackend {
    fn play(&self, path: &Path, looped: bool) -> Result<(), NativeEffectError> {
        let wide = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let mut flags = SND_FILENAME | SND_ASYNC | SND_NODEFAULT;
        if looped {
            flags |= SND_LOOP;
        }
        unsafe { PlaySoundW(PCWSTR(wide.as_ptr()), None, flags) }
            .as_bool()
            .then_some(())
            .ok_or(NativeEffectError)
    }

    fn stop(&self) -> Result<(), NativeEffectError> {
        unsafe { PlaySoundW(PCWSTR::null(), None, SND_FLAGS(0)) }
            .as_bool()
            .then_some(())
            .ok_or(NativeEffectError)
    }
}

struct WindowsAudioPort<B: SoundBackend> {
    path: PathBuf,
    backend: B,
}

impl<B: SoundBackend> AttentionAudioPort for WindowsAudioPort<B> {
    fn play_ordinary(&self) -> Result<(), NativeEffectError> {
        self.backend.play(&self.path, false)
    }

    fn stop_ordinary(&self) -> Result<(), NativeEffectError> {
        self.backend.stop()
    }

    fn start_timer_loop(&self, _ticket: &AudioTicket) -> Result<bool, NativeEffectError> {
        self.backend.play(&self.path, true).map(|()| true)
    }

    fn stop_timer_loop(&self, _ticket: &AudioTicket) -> Result<(), NativeEffectError> {
        self.backend.stop()
    }

    fn shutdown(&self) {
        let _ = self.backend.stop();
    }
}

pub(crate) fn windows_audio(path: PathBuf) -> Arc<dyn AttentionAudioPort> {
    Arc::new(WindowsAudioPort {
        path,
        backend: WindowsSoundBackend,
    })
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Mutex};

    use sha2::{Digest, Sha256};

    use super::{SoundBackend, WindowsAudioPort};
    use crate::{
        message_center::NativeEffectError,
        native_attention::AttentionAudioPort,
        public_plugins::{AudioTicket, TimerKey},
    };

    #[derive(Default)]
    struct FakeBackend(Mutex<Vec<&'static str>>);

    impl SoundBackend for FakeBackend {
        fn play(&self, _path: &Path, looped: bool) -> Result<(), NativeEffectError> {
            self.0
                .lock()
                .unwrap()
                .push(if looped { "loop" } else { "once" });
            Ok(())
        }

        fn stop(&self) -> Result<(), NativeEffectError> {
            self.0.lock().unwrap().push("stop");
            Ok(())
        }
    }

    fn ticket() -> AudioTicket {
        AudioTicket {
            key: TimerKey::new("com.example.audio", 1, 1).unwrap(),
            round_id: 1,
            audio_id: 1,
            fired_revision: 1,
        }
    }

    #[test]
    fn one_shot_loop_and_stop_share_one_backend() {
        let port = WindowsAudioPort {
            path: "attention.wav".into(),
            backend: FakeBackend::default(),
        };

        port.play_ordinary().unwrap();
        assert!(port.start_timer_loop(&ticket()).unwrap());
        port.stop_timer_loop(&ticket()).unwrap();

        assert_eq!(*port.backend.0.lock().unwrap(), ["once", "loop", "stop"]);
    }

    #[test]
    fn bundled_attention_wave_has_the_approved_identity_and_resource_path() {
        let bytes = include_bytes!("../../resources/sounds/attention-alarm.wav");
        assert_eq!(bytes.len(), 1_724_844);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(
            format!("{:X}", Sha256::digest(bytes)),
            "9F66E473EEEE7AAF75AB2761423DAD1D04FA3F019744899DD154350F4117A8F3"
        );
        let config = include_str!("../../tauri.conf.json");
        assert!(config.contains("resources/sounds/attention-alarm.wav"));
        assert!(!config.contains("resources/sounds/timer-complete.wav"));
    }
}
