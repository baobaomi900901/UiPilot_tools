//! Host-owned Panel text effects. Native speech objects never leave the worker thread.
use serde::{Deserialize, Serialize};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub(crate) struct EffectLease(Arc<Mutex<bool>>);
impl EffectLease {
    pub(crate) fn new() -> Self {
        Self(Arc::new(Mutex::new(true)))
    }
    pub(crate) fn revoke(&self) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = false;
    }
    pub(crate) fn valid(&self) -> bool {
        self.0.lock().map(|v| *v).unwrap_or(false)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum SpeechLanguage {
    #[serde(rename = "en")]
    English,
    #[serde(rename = "zh-CHS")]
    Chinese,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpeechVoice {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) language: SpeechLanguage,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpeechState {
    pub(crate) playback_id: Option<String>,
    pub(crate) status: &'static str,
    pub(crate) language: Option<SpeechLanguage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error_code: Option<&'static str>,
}
impl SpeechState {
    fn idle() -> Self {
        Self {
            playback_id: None,
            status: "idle",
            language: None,
            error_code: None,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EffectError {
    PermissionDenied,
    ExpiredPanelSession,
    InvalidPanelText,
    ClipboardWriteFailed,
    SpeechUnavailable,
    SpeechBusy,
    SpeechVoiceUnavailable,
    SpeechFailed,
}
impl EffectError {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::PermissionDenied => "PermissionDenied",
            Self::ExpiredPanelSession => "ExpiredPanelSession",
            Self::InvalidPanelText => "InvalidPanelText",
            Self::ClipboardWriteFailed => "ClipboardWriteFailed",
            Self::SpeechUnavailable => "SpeechUnavailable",
            Self::SpeechBusy => "SpeechBusy",
            Self::SpeechVoiceUnavailable => "SpeechVoiceUnavailable",
            Self::SpeechFailed => "SpeechFailed",
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PanelText {
    pub(crate) text: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SpeechPlay {
    pub(crate) text: String,
    pub(crate) language: SpeechLanguage,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SpeechStop {
    pub(crate) playback_id: String,
}
pub(crate) fn validate_text(text: &str) -> Result<(), EffectError> {
    if text.is_empty() || text.len() > 16384 || text.contains('\0') {
        Err(EffectError::InvalidPanelText)
    } else {
        Ok(())
    }
}
#[derive(Clone)]
pub(crate) struct SpeechOwner {
    pub(crate) key: String,
    pub(crate) session: EffectLease,
    pub(crate) authorization: EffectLease,
    pub(crate) publish: Arc<dyn Fn(SpeechState) + Send + Sync>,
}
impl SpeechOwner {
    fn guarded<T>(&self, effect: impl FnOnce() -> T) -> Result<T, EffectError> {
        // Fixed order, no manager/controller locks or awaits inside this gate.
        let session = self
            .session
            .0
            .lock()
            .map_err(|_| EffectError::ExpiredPanelSession)?;
        let authorization = self
            .authorization
            .0
            .lock()
            .map_err(|_| EffectError::ExpiredPanelSession)?;
        if !*session || !*authorization {
            return Err(EffectError::ExpiredPanelSession);
        }
        Ok(effect())
    }
    fn valid(&self) -> bool {
        self.session.valid() && self.authorization.valid()
    }
    fn publish(&self, state: SpeechState) {
        let _ = self.guarded(|| (self.publish)(state));
    }
}
trait SpeechDriver {
    fn voices(&mut self) -> Result<Vec<SpeechVoice>, EffectError>;
    fn start(&mut self, text: &str, language: SpeechLanguage) -> Result<u32, EffectError>;
    fn stop(&mut self) -> Result<(), EffectError>;
    fn finished(&mut self, stream: u32) -> Result<bool, EffectError>;
}
struct Playback {
    owner: SpeechOwner,
    state: SpeechState,
    stream: u32,
}
struct SpeechWorker<D> {
    driver: D,
    current: Option<Playback>,
    next: u64,
}
impl<D: SpeechDriver> SpeechWorker<D> {
    fn new(driver: D) -> Self {
        Self {
            driver,
            current: None,
            next: 0,
        }
    }
    fn poll(&mut self) {
        let Some(current) = self.current.as_ref() else {
            return;
        };
        if !current.owner.valid() {
            if self.driver.stop().is_ok() {
                self.current = None;
            }
            return;
        }
        if current.state.status != "playing" {
            return;
        }
        let result = self.driver.finished(current.stream);
        if result == Ok(false) {
            return;
        }
        let current = self.current.as_mut().unwrap();
        current.state.status = if result.is_ok() { "completed" } else { "error" };
        current.state.error_code = result.err().map(EffectError::code);
        current.owner.publish(current.state.clone());
    }
    fn state(&self, owner: &SpeechOwner) -> Result<SpeechState, EffectError> {
        owner.guarded(|| {
            self.current
                .as_ref()
                .filter(|p| p.owner.key == owner.key)
                .map(|p| p.state.clone())
                .unwrap_or_else(SpeechState::idle)
        })
    }
    fn play(&mut self, owner: SpeechOwner, input: SpeechPlay) -> Result<SpeechState, EffectError> {
        validate_text(&input.text)?;
        owner.guarded(|| ())?;
        if self.current.is_some() {
            self.driver.stop()?;
        }
        if let Some(mut previous) = self.current.take() {
            if previous.state.status == "playing" {
                previous.state.status = "stopped";
                previous.owner.publish(previous.state);
            }
        }
        self.next = self
            .next
            .checked_add(1)
            .ok_or(EffectError::SpeechUnavailable)?;
        let mut state = SpeechState {
            playback_id: Some(format!("speech-{}", self.next)),
            status: "playing",
            language: Some(input.language),
            error_code: None,
        };
        // Final lease check and native start are serialized with synchronous revoke.
        let result = owner.guarded(|| self.driver.start(&input.text, input.language))?;
        match result {
            Ok(stream) => {
                self.current = Some(Playback {
                    owner: owner.clone(),
                    state: state.clone(),
                    stream,
                });
                owner.publish(state.clone());
                Ok(state)
            }
            Err(error) => {
                let _ = self.driver.stop();
                state.status = "error";
                state.error_code = Some(error.code());
                owner.publish(state.clone());
                self.current = Some(Playback {
                    owner,
                    state,
                    stream: 0,
                });
                Err(error)
            }
        }
    }
    fn stop(&mut self, owner: &SpeechOwner, id: &str) -> Result<SpeechState, EffectError> {
        owner.guarded(|| ())?;
        if let Some(current) = self.current.as_mut().filter(|p| {
            p.owner.key == owner.key
                && p.state.playback_id.as_deref() == Some(id)
                && matches!(p.state.status, "playing" | "error")
        }) {
            self.driver.stop()?;
            current.state.status = "stopped";
            current.owner.publish(current.state.clone());
        }
        self.state(owner)
    }
}
type Reply<T> = tokio::sync::oneshot::Sender<Result<T, EffectError>>;
enum SpeechCommand {
    Voices(SpeechOwner, Reply<Vec<SpeechVoice>>),
    Play(SpeechOwner, SpeechPlay, Reply<SpeechState>),
    Stop(SpeechOwner, String, Reply<SpeechState>),
    State(SpeechOwner, Reply<SpeechState>),
}
pub(crate) struct PanelSpeech {
    sender: mpsc::SyncSender<SpeechCommand>,
}
impl PanelSpeech {
    fn start() -> Self {
        let (sender, receiver) = mpsc::sync_channel(16);
        std::thread::Builder::new()
            .name("panel-native-speech".into())
            .spawn(move || {
                let mut worker = SpeechWorker::new(native::NativeSpeech::new());
                loop {
                    worker.poll();
                    // Revocation does not enqueue commands and cannot be blocked by a full user queue.
                    match receiver.recv_timeout(Duration::from_millis(20)) {
                        Ok(SpeechCommand::Voices(owner, reply)) => {
                            let result = owner.guarded(|| worker.driver.voices()).and_then(|r| r);
                            let _ = reply.send(result);
                        }
                        Ok(SpeechCommand::Play(owner, input, reply)) => {
                            let _ = reply.send(worker.play(owner, input));
                        }
                        Ok(SpeechCommand::Stop(owner, id, reply)) => {
                            let _ = reply.send(worker.stop(&owner, &id));
                        }
                        Ok(SpeechCommand::State(owner, reply)) => {
                            let _ = reply.send(worker.state(&owner));
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => (),
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            let _ = worker.driver.stop();
                            break;
                        }
                    }
                }
            })
            .ok();
        Self { sender }
    }
    pub(crate) fn global() -> &'static Self {
        static INSTANCE: OnceLock<PanelSpeech> = OnceLock::new();
        INSTANCE.get_or_init(Self::start)
    }
    fn send(&self, command: SpeechCommand) -> Result<(), EffectError> {
        self.sender.try_send(command).map_err(|e| match e {
            mpsc::TrySendError::Full(_) => EffectError::SpeechBusy,
            _ => EffectError::SpeechUnavailable,
        })
    }
    async fn receive<T>(
        owner: &SpeechOwner,
        reply: tokio::sync::oneshot::Receiver<Result<T, EffectError>>,
    ) -> Result<T, EffectError> {
        let result = reply.await.unwrap_or(Err(EffectError::SpeechUnavailable));
        owner.guarded(|| result)?
    }
    pub(crate) async fn voices(&self, owner: SpeechOwner) -> Result<Vec<SpeechVoice>, EffectError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.send(SpeechCommand::Voices(owner.clone(), tx))?;
        Self::receive(&owner, rx).await
    }
    pub(crate) async fn play(
        &self,
        owner: SpeechOwner,
        input: SpeechPlay,
    ) -> Result<SpeechState, EffectError> {
        validate_text(&input.text)?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.send(SpeechCommand::Play(owner.clone(), input, tx))?;
        Self::receive(&owner, rx).await
    }
    pub(crate) async fn stop(
        &self,
        owner: SpeechOwner,
        input: SpeechStop,
    ) -> Result<SpeechState, EffectError> {
        if input.playback_id.is_empty() || input.playback_id.len() > 128 {
            return Err(EffectError::InvalidPanelText);
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.send(SpeechCommand::Stop(owner.clone(), input.playback_id, tx))?;
        Self::receive(&owner, rx).await
    }
    pub(crate) async fn state(&self, owner: SpeechOwner) -> Result<SpeechState, EffectError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.send(SpeechCommand::State(owner.clone(), tx))?;
        Self::receive(&owner, rx).await
    }
}
mod native;
#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Fake {
        starts: Vec<String>,
        stops: usize,
        next: u32,
        complete: Option<u32>,
        fail: bool,
        fail_stop: bool,
    }
    impl SpeechDriver for Fake {
        fn voices(&mut self) -> Result<Vec<SpeechVoice>, EffectError> {
            Ok(vec![])
        }
        fn start(&mut self, text: &str, _: SpeechLanguage) -> Result<u32, EffectError> {
            self.next += 1;
            self.starts.push(text.into());
            Ok(self.next)
        }
        fn stop(&mut self) -> Result<(), EffectError> {
            self.stops += 1;
            if self.fail_stop {
                Err(EffectError::SpeechFailed)
            } else {
                Ok(())
            }
        }
        fn finished(&mut self, id: u32) -> Result<bool, EffectError> {
            if self.fail {
                Err(EffectError::SpeechFailed)
            } else {
                Ok(self.complete == Some(id))
            }
        }
    }
    fn owner(key: &str) -> SpeechOwner {
        SpeechOwner {
            key: key.into(),
            session: EffectLease::new(),
            authorization: EffectLease::new(),
            publish: Arc::new(|_| {}),
        }
    }
    fn input() -> SpeechPlay {
        SpeechPlay {
            text: "hello".into(),
            language: SpeechLanguage::English,
        }
    }
    #[test]
    fn speech_failed_replacement_stop_keeps_native_owner_tracked() {
        let mut worker = SpeechWorker::new(Fake::default());
        let a = owner("a");
        worker.play(a.clone(), input()).unwrap();
        worker.driver.fail_stop = true;
        assert_eq!(
            worker.play(owner("b"), input()),
            Err(EffectError::SpeechFailed)
        );
        assert!(worker.current.is_some());
        assert_eq!(worker.driver.starts.len(), 1);
        worker.driver.fail_stop = false;
        a.session.revoke();
        worker.poll();
        assert_eq!(worker.driver.stops, 2);
    }
    #[tokio::test]
    async fn speech_revocation_before_async_delivery_rejects_the_late_reply() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let service = PanelSpeech { sender };
        let a = owner("a");
        let call = service.state(a.clone());
        tokio::pin!(call);
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(call.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        let SpeechCommand::State(_, reply) = receiver.recv().unwrap() else {
            panic!()
        };
        a.authorization.revoke();
        reply.send(Ok(SpeechState::idle())).unwrap();
        assert_eq!(call.await, Err(EffectError::ExpiredPanelSession));
    }
    #[test]
    fn speech_user_queue_is_bounded_and_revocation_needs_no_queue_slot() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        let service = PanelSpeech { sender };
        let a = owner("a");
        let (tx, _) = tokio::sync::oneshot::channel();
        assert!(service.send(SpeechCommand::State(a.clone(), tx)).is_ok());
        let (tx, _) = tokio::sync::oneshot::channel();
        assert_eq!(
            service.send(SpeechCommand::State(a.clone(), tx)),
            Err(EffectError::SpeechBusy)
        );
        a.session.revoke();
        assert!(!a.valid());
    }
    #[test]
    fn text_contract_preserves_whitespace_and_bounds_utf8() {
        assert_eq!(validate_text(" \n "), Ok(()));
        for value in ["".into(), "a\0b".into(), "中".repeat(5462)] {
            assert_eq!(validate_text(&value), Err(EffectError::InvalidPanelText));
        }
        assert!(serde_json::from_str::<SpeechPlay>(
            r#"{"text":"a","language":"en","voiceId":"x"}"#
        )
        .is_err());
    }
    #[test]
    fn speech_starts_and_old_completion_or_stop_cannot_change_new_playback() {
        let mut worker = SpeechWorker::new(Fake::default());
        let a = owner("a");
        let b = owner("b");
        let first = worker.play(a.clone(), input()).unwrap();
        let second = worker.play(b.clone(), input()).unwrap();
        assert_ne!(first.playback_id, second.playback_id);
        assert_eq!(worker.driver.stops, 1);
        worker.driver.complete = Some(1);
        worker.poll();
        assert_eq!(worker.state(&b).unwrap().status, "playing");
        worker
            .stop(&a, first.playback_id.as_deref().unwrap())
            .unwrap();
        assert_eq!(worker.driver.stops, 1);
        worker.driver.complete = Some(2);
        worker.poll();
        assert_eq!(worker.state(&b).unwrap().status, "completed");
        let old = worker.play(b.clone(), input()).unwrap();
        let current = worker.play(b.clone(), input()).unwrap();
        let stops = worker.driver.stops;
        assert_eq!(worker.stop(&b, old.playback_id.as_deref().unwrap()).unwrap(), current);
        assert_eq!(worker.driver.stops, stops);
    }
    #[test]
    fn speech_revoked_queued_play_cannot_start_and_active_play_stops() {
        for session in [true, false] {
            let mut worker = SpeechWorker::new(Fake::default());
            let a = owner("a");
            if session {
                a.session.revoke()
            } else {
                a.authorization.revoke()
            }
            assert_eq!(
                worker.play(a, input()),
                Err(EffectError::ExpiredPanelSession)
            );
            assert!(worker.driver.starts.is_empty());
            let b = owner("b");
            worker.play(b.clone(), input()).unwrap();
            b.authorization.revoke();
            worker.poll();
            assert_eq!(worker.driver.stops, 1);
            assert!(worker.current.is_none());
        }
    }
    #[test]
    fn speech_native_failure_publishes_error_and_owner_state_is_private() {
        let mut worker = SpeechWorker::new(Fake::default());
        let a = owner("a");
        worker.play(a.clone(), input()).unwrap();
        worker.driver.fail = true;
        worker.poll();
        assert_eq!(worker.state(&a).unwrap().error_code, Some("SpeechFailed"));
        assert_eq!(worker.state(&owner("b")).unwrap(), SpeechState::idle());
    }
}
