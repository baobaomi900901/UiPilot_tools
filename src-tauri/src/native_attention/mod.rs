use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    panic::{self, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
    time::Instant,
};

use crate::{
    message_center::{MessagePublished, MessageToast, NativeEffectError},
    public_plugins::{AudioTicket, PluginTimerService, TimerAudioCompletion, TimerError, TimerKey},
};

use tray::TrayAnimation;
pub(crate) use tray::{TrayAttentionPort, TrayVisual};

mod tray;
mod windows_audio;
mod windows_identity;
mod windows_toast;

pub(crate) fn prepare_process_identity() {
    let _ = windows_identity::prepare_process_identity();
}

pub(crate) fn windows_toast() -> Arc<dyn MessageToast> {
    Arc::new(windows_toast::WindowsToastPort::new())
}

pub(crate) fn tauri_tray<R: tauri::Runtime>(
    tray: tauri::tray::TrayIcon<R>,
    normal: tauri::image::Image<'static>,
) -> Arc<dyn TrayAttentionPort> {
    tray::tauri_tray_port(tray, normal)
}

pub(crate) fn windows_audio(message_path: std::path::PathBuf) -> Arc<dyn AttentionAudioPort> {
    windows_audio::windows_audio(message_path)
}

const ORDINARY_CAPACITY: usize = 64;
const TIMER_KEY_CAPACITY: usize = 64;
const FOCUS_CAPACITY: usize = 128;
const TOAST_CALLBACK_CAPACITY: usize = 64;
const ACTIVE_TOAST_CAPACITY: usize = 64;

pub(crate) type NativeNotificationId = u64;
pub(crate) type ToastCallbackSink =
    Arc<dyn Fn(NativeNotificationId, ToastCallbackKind) + Send + Sync>;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct AlarmPlaybackKey(u64);

impl AlarmPlaybackKey {
    fn from_epoch(epoch: u64) -> Option<Self> {
        (epoch > 0).then_some(Self(epoch))
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(value: u64) -> Self {
        Self::from_epoch(value).expect("test playback key must be nonzero")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublishedAttention {
    pub(crate) message: MessagePublished,
    pub(crate) origin: AttentionOrigin,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AttentionOrigin {
    Ordinary,
    TimerCompletion {
        audio: Option<Box<TimerAudioCompletion>>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ToastCallbackKind {
    Activated,
    Failed,
    Dismissed,
}

pub(crate) trait AttentionAudioPort: Send + Sync {
    fn play_ordinary(&self) -> Result<(), NativeEffectError>;
    fn stop_ordinary(&self) -> Result<(), NativeEffectError>;
    fn start_timer_loop(
        &self,
        key: AlarmPlaybackKey,
        bytes: Arc<[u8]>,
    ) -> Result<(), NativeEffectError>;
    fn stop_timer_loop(&self, key: AlarmPlaybackKey) -> Result<(), NativeEffectError>;
    fn shutdown(&self);
}

pub(crate) trait AttentionRoutePort: Send + Sync {
    fn show_messages(&self);
}

struct CallbackAttentionRoute {
    callback: Arc<dyn Fn() + Send + Sync>,
}

impl AttentionRoutePort for CallbackAttentionRoute {
    fn show_messages(&self) {
        (self.callback)();
    }
}

pub(crate) fn attention_route(
    callback: Arc<dyn Fn() + Send + Sync>,
) -> Arc<dyn AttentionRoutePort> {
    Arc::new(CallbackAttentionRoute { callback })
}

trait TimerAttentionPort: Send + Sync {
    fn admit_audio_start(&self, ticket: &AudioTicket) -> Result<bool, TimerError>;
    fn confirm_audio_without_start(&self, ticket: &AudioTicket) -> Result<bool, TimerError>;
    fn confirm_audio_after_play_failure(&self, ticket: &AudioTicket) -> Result<bool, TimerError>;
    fn linearize_focused(&self, admission: &mut dyn FnMut()) -> Result<(), TimerError>;
    fn terminate_all_audio(&self) -> Result<(), TimerError>;
}

struct PluginTimerAttentionPort {
    timers: Arc<PluginTimerService>,
}

impl TimerAttentionPort for PluginTimerAttentionPort {
    fn admit_audio_start(&self, ticket: &AudioTicket) -> Result<bool, TimerError> {
        self.timers.admit_audio_start(ticket)
    }

    fn confirm_audio_without_start(&self, ticket: &AudioTicket) -> Result<bool, TimerError> {
        self.timers.confirm_audio_without_start(ticket)
    }

    fn confirm_audio_after_play_failure(&self, ticket: &AudioTicket) -> Result<bool, TimerError> {
        self.timers.confirm_audio_after_play_failure(ticket)
    }

    fn linearize_focused(&self, admission: &mut dyn FnMut()) -> Result<(), TimerError> {
        self.timers.with_audio_focus_authority(|authority| {
            admission();
            authority.confirm_all_current();
        })
    }

    fn terminate_all_audio(&self) -> Result<(), TimerError> {
        self.timers.terminate_all_audio()
    }
}

#[derive(Clone, Debug)]
struct Sequenced<T> {
    sequence: u64,
    value: T,
}

#[derive(Clone, Debug)]
struct SequencedPublished {
    attention: PublishedAttention,
    focused_at_admission: bool,
    alarm_epoch: Option<u64>,
}

#[derive(Default)]
struct TimerSlots {
    published: Option<Sequenced<SequencedPublished>>,
    cancelled: Option<Sequenced<TimerCancellation>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TimerAudioOwnerPhase {
    Reserved,
    Playing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TimerAudioOwner {
    epoch: u64,
    ticket: AudioTicket,
    alarm: crate::public_plugins::ValidatedAlarmAsset,
    playback_key: AlarmPlaybackKey,
    phase: TimerAudioOwnerPhase,
}

#[derive(Clone, Debug)]
struct TimerCancellation {
    ticket: AudioTicket,
    stop_key: Option<AlarmPlaybackKey>,
}

#[derive(Clone, Copy, Debug)]
struct FocusEvent {
    focused: bool,
    stop_key: Option<AlarmPlaybackKey>,
}

struct MailboxState {
    next_sequence: u64,
    alarm_epoch: u64,
    alarm_epoch_claimed: bool,
    timer_audio_terminal: bool,
    timer_audio_owner: Option<TimerAudioOwner>,
    main_focused: bool,
    terminal: bool,
    ordinary: VecDeque<Sequenced<SequencedPublished>>,
    timers: BTreeMap<TimerKey, TimerSlots>,
    focus: VecDeque<Sequenced<FocusEvent>>,
    callbacks: BTreeMap<NativeNotificationId, Sequenced<ToastCallbackKind>>,
    toast_ids: BTreeSet<NativeNotificationId>,
}

impl Default for MailboxState {
    fn default() -> Self {
        Self {
            next_sequence: 0,
            alarm_epoch: 1,
            alarm_epoch_claimed: false,
            timer_audio_terminal: false,
            timer_audio_owner: None,
            main_focused: false,
            terminal: false,
            ordinary: VecDeque::new(),
            timers: BTreeMap::new(),
            focus: VecDeque::new(),
            callbacks: BTreeMap::new(),
            toast_ids: BTreeSet::new(),
        }
    }
}

impl MailboxState {
    fn allocate_sequence(&mut self) -> Option<u64> {
        let sequence = self.next_sequence.checked_add(1)?;
        self.next_sequence = sequence;
        Some(sequence)
    }

    fn advance_alarm_epoch(&mut self) {
        self.timer_audio_owner = None;
        self.alarm_epoch_claimed = false;
        match self.alarm_epoch.checked_add(1) {
            Some(epoch) => self.alarm_epoch = epoch,
            None => {
                self.timer_audio_terminal = true;
                self.alarm_epoch_claimed = true;
            }
        }
    }

    fn revoke_timer_owner(
        &mut self,
        ticket: Option<&AudioTicket>,
        advance_without_owner: bool,
    ) -> Option<AlarmPlaybackKey> {
        let matches = self
            .timer_audio_owner
            .as_ref()
            .is_some_and(|owner| ticket.is_none_or(|ticket| owner.ticket == *ticket));
        if !matches && !advance_without_owner {
            return None;
        }
        let stop_key = self.timer_audio_owner.as_ref().and_then(|owner| {
            (owner.phase == TimerAudioOwnerPhase::Playing).then_some(owner.playback_key)
        });
        self.advance_alarm_epoch();
        stop_key
    }

    fn next_event(&mut self) -> Option<WorkerEvent> {
        let mut candidate: Option<(u64, EventLocation)> = None;
        let mut consider = |sequence: u64, location: EventLocation| {
            if candidate
                .as_ref()
                .is_none_or(|(current, _)| sequence < *current)
            {
                candidate = Some((sequence, location));
            }
        };

        if let Some(event) = self.ordinary.front() {
            consider(event.sequence, EventLocation::Ordinary);
        }
        if let Some(event) = self.focus.front() {
            consider(event.sequence, EventLocation::Focus);
        }
        for (key, slots) in &self.timers {
            if let Some(event) = slots.published.as_ref() {
                consider(event.sequence, EventLocation::TimerPublished(key.clone()));
            }
            if let Some(event) = slots.cancelled.as_ref() {
                consider(event.sequence, EventLocation::TimerCancelled(key.clone()));
            }
        }
        for (id, event) in &self.callbacks {
            consider(event.sequence, EventLocation::ToastCallback(*id));
        }

        let (_, location) = candidate?;
        match location {
            EventLocation::Ordinary => self
                .ordinary
                .pop_front()
                .map(|event| WorkerEvent::Published(event.value)),
            EventLocation::Focus => self
                .focus
                .pop_front()
                .map(|event| WorkerEvent::MainFocusChanged(event.value)),
            EventLocation::TimerPublished(key) => {
                let slots = self.timers.get_mut(&key)?;
                let event = slots.published.take()?;
                if slots.cancelled.is_none() {
                    self.timers.remove(&key);
                }
                Some(WorkerEvent::Published(event.value))
            }
            EventLocation::TimerCancelled(key) => {
                let slots = self.timers.get_mut(&key)?;
                let event = slots.cancelled.take()?;
                if slots.published.is_none() {
                    self.timers.remove(&key);
                }
                Some(WorkerEvent::TimerAudioCancelled(event.value))
            }
            EventLocation::ToastCallback(id) => {
                self.callbacks
                    .remove(&id)
                    .map(|event| WorkerEvent::ToastCallback {
                        notification_id: id,
                        kind: event.value,
                    })
            }
        }
    }

    fn has_events(&self) -> bool {
        !self.ordinary.is_empty()
            || !self.timers.is_empty()
            || !self.focus.is_empty()
            || !self.callbacks.is_empty()
    }
}

enum EventLocation {
    Ordinary,
    Focus,
    TimerPublished(TimerKey),
    TimerCancelled(TimerKey),
    ToastCallback(NativeNotificationId),
}

enum WorkerEvent {
    Published(SequencedPublished),
    TimerAudioCancelled(TimerCancellation),
    MainFocusChanged(FocusEvent),
    ToastCallback {
        notification_id: NativeNotificationId,
        kind: ToastCallbackKind,
    },
    TrayTick,
}

struct Mailbox {
    state: Mutex<MailboxState>,
    wake: Condvar,
    shutdown: AtomicBool,
}

impl Default for Mailbox {
    fn default() -> Self {
        Self {
            state: Mutex::new(MailboxState::default()),
            wake: Condvar::new(),
            shutdown: AtomicBool::new(false),
        }
    }
}

struct Ports {
    toast: Arc<dyn MessageToast>,
    tray: Arc<dyn TrayAttentionPort>,
    audio: Arc<dyn AttentionAudioPort>,
    route: Arc<dyn AttentionRoutePort>,
    timers: Arc<dyn TimerAttentionPort>,
}

#[derive(Default)]
struct WorkerState {
    tray: TrayAnimation,
    ordinary_playing: bool,
    active_toasts: BTreeSet<NativeNotificationId>,
}

pub(crate) struct NativeAttentionCoordinator {
    mailbox: Arc<Mailbox>,
    ports: Arc<Ports>,
    worker: Mutex<Option<JoinHandle<()>>>,
    worker_alive: Arc<AtomicBool>,
    emergency_started: AtomicBool,
}

impl NativeAttentionCoordinator {
    pub(crate) fn start(
        timers: Arc<PluginTimerService>,
        toast: Arc<dyn MessageToast>,
        tray: Arc<dyn TrayAttentionPort>,
        audio: Arc<dyn AttentionAudioPort>,
        route: Arc<dyn AttentionRoutePort>,
    ) -> Arc<Self> {
        Self::start_with_timer_port(
            Arc::new(PluginTimerAttentionPort { timers }),
            toast,
            tray,
            audio,
            route,
        )
    }

    fn start_with_timer_port(
        timers: Arc<dyn TimerAttentionPort>,
        toast: Arc<dyn MessageToast>,
        tray: Arc<dyn TrayAttentionPort>,
        audio: Arc<dyn AttentionAudioPort>,
        route: Arc<dyn AttentionRoutePort>,
    ) -> Arc<Self> {
        let mailbox = Arc::new(Mailbox::default());
        let ports = Arc::new(Ports {
            toast,
            tray,
            audio,
            route,
            timers,
        });
        let worker_alive = Arc::new(AtomicBool::new(true));
        let coordinator = Arc::new(Self {
            mailbox: Arc::clone(&mailbox),
            ports: Arc::clone(&ports),
            worker: Mutex::new(None),
            worker_alive: Arc::clone(&worker_alive),
            emergency_started: AtomicBool::new(false),
        });
        let weak = Arc::downgrade(&coordinator);
        let _ = coordinator.ports.toast.install_callback_sink(Arc::new(
            move |notification_id, kind| {
                if let Some(coordinator) = weak.upgrade() {
                    coordinator.toast_callback(notification_id, kind);
                }
            },
        ));
        let worker = thread::Builder::new()
            .name("uipilot-native-attention".into())
            .spawn(move || {
                let result = panic::catch_unwind(AssertUnwindSafe(|| {
                    worker_loop(Arc::clone(&mailbox), Arc::clone(&ports));
                }));
                worker_alive.store(false, Ordering::Release);
                if result.is_err() {
                    if let Ok(mut state) = mailbox.state.lock() {
                        state.terminal = true;
                    }
                    mailbox.shutdown.store(true, Ordering::Release);
                    mailbox.wake.notify_all();
                    emergency_stop(&ports);
                }
            });
        match worker {
            Ok(worker) => {
                *coordinator
                    .worker
                    .lock()
                    .expect("attention worker lock poisoned") = Some(worker);
            }
            Err(_) => {
                coordinator.worker_alive.store(false, Ordering::Release);
                coordinator.enter_terminal();
            }
        }
        coordinator
    }

    pub(crate) fn publish(&self, attention: PublishedAttention) {
        if !self.worker_alive.load(Ordering::Acquire) {
            self.enter_terminal();
            self.terminate_published_ticket(&attention);
            return;
        }
        match attention.origin.clone() {
            AttentionOrigin::Ordinary => self.publish_ordinary(attention),
            AttentionOrigin::TimerCompletion { .. } => self.publish_timer(attention),
        }
    }

    fn publish_ordinary(&self, attention: PublishedAttention) {
        let admitted = {
            let Ok(mut state) = self.mailbox.state.lock() else {
                self.enter_terminal();
                return;
            };
            if state.terminal || state.ordinary.len() >= ORDINARY_CAPACITY {
                return;
            }
            let focused_at_admission = state.main_focused;
            let Some(sequence) = state.allocate_sequence() else {
                state.terminal = true;
                drop(state);
                self.enter_terminal();
                return;
            };
            state.ordinary.push_back(Sequenced {
                sequence,
                value: SequencedPublished {
                    attention,
                    focused_at_admission,
                    alarm_epoch: None,
                },
            });
            true
        };
        if admitted {
            self.mailbox.wake.notify_one();
        }
    }

    fn publish_timer(&self, attention: PublishedAttention) {
        let key = match &attention.origin {
            AttentionOrigin::TimerCompletion { audio: Some(audio) } => {
                Some(audio.ticket.key().clone())
            }
            AttentionOrigin::TimerCompletion { audio: None } => None,
            AttentionOrigin::Ordinary => return,
        };
        let mut replaced_ticket = None;
        let mut rejected_ticket = None;
        let admitted = {
            let Ok(mut state) = self.mailbox.state.lock() else {
                self.enter_terminal();
                self.terminate_published_ticket(&attention);
                return;
            };
            if state.terminal {
                rejected_ticket = timer_ticket(&attention).cloned();
                false
            } else {
                let slot_key = key.clone().unwrap_or_else(|| timerless_key(&attention));
                if !state.timers.contains_key(&slot_key) && state.timers.len() >= TIMER_KEY_CAPACITY
                {
                    rejected_ticket = timer_ticket(&attention).cloned();
                    false
                } else {
                    let focused_at_admission = state.main_focused;
                    let Some(sequence) = state.allocate_sequence() else {
                        state.terminal = true;
                        rejected_ticket = timer_ticket(&attention).cloned();
                        drop(state);
                        self.enter_terminal();
                        if let Some(ticket) = rejected_ticket {
                            let _ = self.ports.timers.confirm_audio_without_start(&ticket);
                        }
                        return;
                    };
                    let alarm_epoch = matches!(
                        &attention.origin,
                        AttentionOrigin::TimerCompletion { audio: Some(_) }
                    )
                    .then_some(state.alarm_epoch)
                    .filter(|_| !state.timer_audio_terminal);
                    let slots = state.timers.entry(slot_key).or_default();
                    replaced_ticket = slots
                        .published
                        .as_ref()
                        .and_then(|event| timer_ticket(&event.value.attention).cloned());
                    slots.published = Some(Sequenced {
                        sequence,
                        value: SequencedPublished {
                            alarm_epoch,
                            attention,
                            focused_at_admission,
                        },
                    });
                    true
                }
            }
        };
        for ticket in replaced_ticket.into_iter().chain(rejected_ticket) {
            let _ = self.ports.timers.confirm_audio_without_start(&ticket);
        }
        if admitted {
            self.mailbox.wake.notify_one();
        }
    }

    pub(crate) fn cancel_timer_audio(&self, ticket: AudioTicket) {
        let key = ticket.key().clone();
        let mut terminal = false;
        let admitted = {
            let Ok(mut state) = self.mailbox.state.lock() else {
                self.enter_terminal();
                return;
            };
            if state.terminal {
                false
            } else if !state.timers.contains_key(&key) && state.timers.len() >= TIMER_KEY_CAPACITY {
                state.terminal = true;
                terminal = true;
                false
            } else {
                let duplicate = state
                    .timers
                    .get(&key)
                    .and_then(|slots| slots.cancelled.as_ref())
                    .is_some_and(|event| event.value.ticket == ticket);
                if duplicate {
                    false
                } else if state
                    .timers
                    .get(&key)
                    .is_some_and(|slots| slots.cancelled.is_some())
                {
                    state.terminal = true;
                    terminal = true;
                    false
                } else if let Some(sequence) = state.allocate_sequence() {
                    let stop_key = state.revoke_timer_owner(Some(&ticket), false);
                    state.timers.entry(key).or_default().cancelled = Some(Sequenced {
                        sequence,
                        value: TimerCancellation { ticket, stop_key },
                    });
                    true
                } else {
                    state.terminal = true;
                    terminal = true;
                    false
                }
            }
        };
        if terminal {
            self.enter_terminal();
        } else if admitted {
            self.mailbox.wake.notify_one();
        }
    }

    pub(crate) fn observe_main_focus(&self, focused: bool) {
        if focused {
            let mut terminal = false;
            let mut admission = || terminal = self.admit_focus_event(true, false);
            let failed = self.ports.timers.linearize_focused(&mut admission).is_err();
            if failed || terminal {
                self.enter_terminal();
            } else {
                self.mailbox.wake.notify_one();
            }
        } else if self.admit_focus_event(false, true) {
            self.enter_terminal();
        }
    }

    fn admit_focus_event(&self, focused: bool, notify_worker: bool) -> bool {
        let mut terminal = false;
        let admitted = {
            let Ok(mut state) = self.mailbox.state.lock() else {
                return true;
            };
            if state.terminal {
                false
            } else if state.focus.len() >= FOCUS_CAPACITY {
                state.terminal = true;
                terminal = true;
                false
            } else if let Some(sequence) = state.allocate_sequence() {
                state.main_focused = focused;
                let stop_key = focused
                    .then(|| state.revoke_timer_owner(None, true))
                    .flatten();
                state.focus.push_back(Sequenced {
                    sequence,
                    value: FocusEvent { focused, stop_key },
                });
                true
            } else {
                state.terminal = true;
                terminal = true;
                false
            }
        };
        if terminal {
            self.mailbox.wake.notify_all();
        } else if admitted && notify_worker {
            self.mailbox.wake.notify_one();
        }
        terminal
    }

    pub(crate) fn toast_callback(
        &self,
        notification_id: NativeNotificationId,
        kind: ToastCallbackKind,
    ) {
        let mut terminal = false;
        let admitted = {
            let Ok(mut state) = self.mailbox.state.lock() else {
                self.enter_terminal();
                return;
            };
            if state.terminal
                || state.callbacks.contains_key(&notification_id)
                || !state.toast_ids.contains(&notification_id)
            {
                false
            } else if state.callbacks.len() >= TOAST_CALLBACK_CAPACITY {
                state.terminal = true;
                terminal = true;
                false
            } else if let Some(sequence) = state.allocate_sequence() {
                state.callbacks.insert(
                    notification_id,
                    Sequenced {
                        sequence,
                        value: kind,
                    },
                );
                true
            } else {
                state.terminal = true;
                terminal = true;
                false
            }
        };
        if terminal {
            self.enter_terminal();
        } else if admitted {
            self.mailbox.wake.notify_one();
        }
    }

    pub(crate) fn shutdown(&self) {
        self.mailbox.shutdown.store(true, Ordering::Release);
        self.mailbox.wake.notify_all();
        if let Some(worker) = self
            .worker
            .lock()
            .expect("attention worker lock poisoned")
            .take()
        {
            let _ = worker.join();
        }
    }

    fn terminate_published_ticket(&self, attention: &PublishedAttention) {
        if let Some(ticket) = timer_ticket(attention) {
            let _ = self.ports.timers.confirm_audio_without_start(ticket);
        }
    }

    fn enter_terminal(&self) {
        if let Ok(mut state) = self.mailbox.state.lock() {
            state.terminal = true;
        }
        self.mailbox.shutdown.store(true, Ordering::Release);
        self.mailbox.wake.notify_all();
        if self
            .emergency_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            emergency_stop(&self.ports);
        }
    }
}

fn worker_loop(mailbox: Arc<Mailbox>, ports: Arc<Ports>) {
    let _cleanup = CleanupGuard {
        ports: Arc::clone(&ports),
    };
    let _ = ports.toast.initialize_worker();
    let mut state = WorkerState::default();
    loop {
        let event = {
            let mut mailbox_state = mailbox.state.lock().expect("attention mailbox poisoned");
            loop {
                if mailbox.shutdown.load(Ordering::Acquire) || mailbox_state.terminal {
                    break None;
                }
                if let Some(event) = mailbox_state.next_event() {
                    break Some(event);
                }
                if let Some(wait) = state.tray.wait_duration(Instant::now()) {
                    let result = mailbox
                        .wake
                        .wait_timeout(mailbox_state, wait)
                        .expect("attention mailbox poisoned");
                    mailbox_state = result.0;
                    if result.1.timed_out() && !mailbox_state.has_events() {
                        break Some(WorkerEvent::TrayTick);
                    }
                } else {
                    mailbox_state = mailbox
                        .wake
                        .wait(mailbox_state)
                        .expect("attention mailbox poisoned");
                }
            }
        };
        let Some(event) = event else {
            break;
        };
        process_event(&mailbox, &ports, &mut state, event);
    }
}

fn process_event(mailbox: &Mailbox, ports: &Ports, state: &mut WorkerState, event: WorkerEvent) {
    match event {
        WorkerEvent::Published(event) => process_published(mailbox, ports, state, event),
        WorkerEvent::TimerAudioCancelled(cancellation) => {
            let _ticket = cancellation.ticket;
            if let Some(key) = cancellation.stop_key {
                let _ = ports.audio.stop_timer_loop(key);
            }
        }
        WorkerEvent::MainFocusChanged(focus) => {
            let tray_action = state.tray.focus(focus.focused);
            apply_tray_transition(ports.tray.as_ref(), &mut state.tray, tray_action);
            if focus.focused {
                if state.ordinary_playing {
                    let _ = ports.audio.stop_ordinary();
                    state.ordinary_playing = false;
                }
                if let Some(key) = focus.stop_key {
                    let _ = ports.audio.stop_timer_loop(key);
                }
            }
        }
        WorkerEvent::TrayTick => {
            let tray_action = state.tray.advance(Instant::now());
            apply_tray_transition(ports.tray.as_ref(), &mut state.tray, tray_action);
        }
        WorkerEvent::ToastCallback {
            notification_id,
            kind,
        } => {
            if state.active_toasts.remove(&notification_id) {
                ports.toast.finish_notification(notification_id);
                release_toast_id(mailbox, notification_id);
                if kind == ToastCallbackKind::Activated {
                    ports.route.show_messages();
                }
            }
        }
    }
}

fn process_published(
    mailbox: &Mailbox,
    ports: &Ports,
    state: &mut WorkerState,
    event: SequencedPublished,
) {
    if event.focused_at_admission {
        if let Some(ticket) = timer_ticket(&event.attention) {
            let _ = ports.timers.confirm_audio_without_start(ticket);
        }
        return;
    }

    if let Ok(id) = event.attention.message.id.parse::<u64>() {
        if reserve_toast_id(mailbox, id) {
            if ports.toast.show_message(&event.attention.message).is_ok() {
                state.active_toasts.insert(id);
            } else {
                release_toast_id(mailbox, id);
            }
        }
    }
    let tray_action = state.tray.activate(Instant::now());
    apply_tray_transition(ports.tray.as_ref(), &mut state.tray, tray_action);

    match event.attention.origin {
        AttentionOrigin::Ordinary => {
            let timer_playing = mailbox.state.lock().is_ok_and(|mailbox| {
                mailbox
                    .timer_audio_owner
                    .as_ref()
                    .is_some_and(|owner| owner.phase == TimerAudioOwnerPhase::Playing)
            });
            if !timer_playing && ports.audio.play_ordinary().is_ok() {
                state.ordinary_playing = true;
            }
        }
        AttentionOrigin::TimerCompletion { audio: Some(audio) } => {
            let TimerAudioCompletion { ticket, alarm } = *audio;
            let Some(epoch) = event.alarm_epoch else {
                let _ = ports.timers.confirm_audio_without_start(&ticket);
                return;
            };
            let epoch_is_open = mailbox.state.lock().is_ok_and(|mailbox| {
                !mailbox.terminal
                    && !mailbox.timer_audio_terminal
                    && mailbox.alarm_epoch == epoch
                    && !mailbox.alarm_epoch_claimed
                    && mailbox.timer_audio_owner.is_none()
            });
            if !epoch_is_open {
                let _ = ports.timers.confirm_audio_without_start(&ticket);
                return;
            }
            if !ports.timers.admit_audio_start(&ticket).unwrap_or(false) {
                return;
            }
            let reserved = mailbox.state.lock().ok().and_then(|mut mailbox| {
                if mailbox.terminal
                    || mailbox.timer_audio_terminal
                    || mailbox.alarm_epoch != epoch
                    || mailbox.alarm_epoch_claimed
                    || mailbox.timer_audio_owner.is_some()
                {
                    return None;
                }
                let playback_key = AlarmPlaybackKey::from_epoch(epoch)?;
                let owner = TimerAudioOwner {
                    epoch,
                    ticket: ticket.clone(),
                    alarm: alarm.clone(),
                    playback_key,
                    phase: TimerAudioOwnerPhase::Reserved,
                };
                mailbox.alarm_epoch_claimed = true;
                mailbox.timer_audio_owner = Some(owner.clone());
                Some(owner)
            });
            let Some(reserved) = reserved else {
                let _ = ports.timers.confirm_audio_after_play_failure(&ticket);
                return;
            };
            if state.ordinary_playing {
                let _ = ports.audio.stop_ordinary();
                state.ordinary_playing = false;
            }
            let started = ports
                .audio
                .start_timer_loop(reserved.playback_key, Arc::clone(&reserved.alarm.bytes));
            if started.is_ok() {
                let committed = mailbox.state.lock().is_ok_and(|mut mailbox| {
                    if !mailbox
                        .timer_audio_owner
                        .as_ref()
                        .is_some_and(|current| timer_owner_matches(current, &reserved))
                    {
                        return false;
                    }
                    mailbox
                        .timer_audio_owner
                        .as_mut()
                        .expect("matching timer owner missing")
                        .phase = TimerAudioOwnerPhase::Playing;
                    true
                });
                if !committed {
                    let _ = ports.audio.stop_timer_loop(reserved.playback_key);
                }
            } else {
                if let Ok(mut mailbox) = mailbox.state.lock() {
                    if mailbox
                        .timer_audio_owner
                        .as_ref()
                        .is_some_and(|current| timer_owner_matches(current, &reserved))
                    {
                        mailbox.advance_alarm_epoch();
                    }
                }
                let _ = ports.timers.confirm_audio_after_play_failure(&ticket);
            }
        }
        AttentionOrigin::TimerCompletion { audio: None } => {}
    }
}

fn apply_tray_transition(
    port: &dyn TrayAttentionPort,
    animation: &mut TrayAnimation,
    action: Option<TrayVisual>,
) {
    if action.is_some_and(|visual| port.set_visual(visual).is_err()) {
        eprintln!("[native-attention] tray update failed");
        if let Some(restore) = animation.degrade() {
            let _ = port.set_visual(restore);
        }
    }
}

fn reserve_toast_id(mailbox: &Mailbox, notification_id: NativeNotificationId) -> bool {
    let Ok(mut state) = mailbox.state.lock() else {
        return false;
    };
    if state.terminal
        || state.toast_ids.len() >= ACTIVE_TOAST_CAPACITY
        || !state.toast_ids.insert(notification_id)
    {
        return false;
    }
    true
}

fn release_toast_id(mailbox: &Mailbox, notification_id: NativeNotificationId) {
    if let Ok(mut state) = mailbox.state.lock() {
        state.toast_ids.remove(&notification_id);
    }
}

fn timer_ticket(attention: &PublishedAttention) -> Option<&AudioTicket> {
    match &attention.origin {
        AttentionOrigin::TimerCompletion { audio } => audio.as_ref().map(|audio| &audio.ticket),
        AttentionOrigin::Ordinary => None,
    }
}

fn timer_owner_matches(current: &TimerAudioOwner, expected: &TimerAudioOwner) -> bool {
    current.epoch == expected.epoch
        && current.ticket == expected.ticket
        && current.alarm.identity == expected.alarm.identity
        && Arc::ptr_eq(&current.alarm.bytes, &expected.alarm.bytes)
        && current.playback_key == expected.playback_key
        && current.phase == expected.phase
}

fn timerless_key(attention: &PublishedAttention) -> TimerKey {
    TimerKey {
        plugin_id: attention.message.plugin_id.clone(),
        plugin_generation: attention.message.id.parse().unwrap_or(1),
        activation_id: attention.message.id.parse().unwrap_or(1),
    }
}

struct CleanupGuard {
    ports: Arc<Ports>,
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        self.ports.audio.shutdown();
        self.ports.tray.shutdown();
        self.ports.toast.shutdown();
        let _ = self.ports.timers.terminate_all_audio();
    }
}

fn emergency_stop(ports: &Ports) {
    ports.audio.shutdown();
    let _ = ports.tray.set_visual(TrayVisual::Normal);
    let _ = ports.timers.terminate_all_audio();
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{Arc, Condvar, Mutex},
    };

    use super::*;
    use crate::public_plugins::{AlarmAssetIdentity, ValidatedAlarmAsset};

    #[derive(Default)]
    struct Calls {
        values: Mutex<Vec<&'static str>>,
        changed: Condvar,
    }

    impl Calls {
        fn push(&self, value: &'static str) {
            self.values.lock().unwrap().push(value);
            self.changed.notify_all();
        }

        fn wait_for(&self, count: usize) -> Vec<&'static str> {
            let values = self.values.lock().unwrap();
            let values = self
                .changed
                .wait_timeout_while(values, std::time::Duration::from_secs(2), |values| {
                    values.len() < count
                })
                .unwrap()
                .0;
            values.clone()
        }
    }

    struct FakeToast(Arc<Calls>);

    impl MessageToast for FakeToast {
        fn show_message(&self, _message: &MessagePublished) -> Result<(), NativeEffectError> {
            self.0.push("toast");
            Ok(())
        }

        fn shutdown(&self) {
            self.0.push("toast-shutdown");
        }
    }

    struct FakeTray(Arc<Calls>);

    impl TrayAttentionPort for FakeTray {
        fn set_visual(&self, visual: TrayVisual) -> Result<(), NativeEffectError> {
            self.0.push(match visual {
                TrayVisual::Normal => "focus",
                TrayVisual::Transparent => "tray",
            });
            Ok(())
        }

        fn shutdown(&self) {
            self.0.push("tray-shutdown");
        }
    }

    struct FakeAudio(Arc<Calls>);

    impl AttentionAudioPort for FakeAudio {
        fn play_ordinary(&self) -> Result<(), NativeEffectError> {
            self.0.push("ordinary");
            Ok(())
        }

        fn stop_ordinary(&self) -> Result<(), NativeEffectError> {
            self.0.push("ordinary-stop");
            Ok(())
        }

        fn start_timer_loop(
            &self,
            _key: AlarmPlaybackKey,
            _bytes: Arc<[u8]>,
        ) -> Result<(), NativeEffectError> {
            self.0.push("timer-start");
            Ok(())
        }

        fn stop_timer_loop(&self, _key: AlarmPlaybackKey) -> Result<(), NativeEffectError> {
            self.0.push("timer-stop");
            Ok(())
        }

        fn shutdown(&self) {
            self.0.push("audio-shutdown");
        }
    }

    struct BlockingAudio {
        calls: Arc<Calls>,
        succeeds: bool,
        released: Mutex<bool>,
        changed: Condvar,
    }

    impl BlockingAudio {
        fn new(calls: Arc<Calls>) -> Arc<Self> {
            Arc::new(Self {
                calls,
                succeeds: true,
                released: Mutex::new(false),
                changed: Condvar::new(),
            })
        }

        fn new_failing(calls: Arc<Calls>) -> Arc<Self> {
            Arc::new(Self {
                calls,
                succeeds: false,
                released: Mutex::new(false),
                changed: Condvar::new(),
            })
        }

        fn release(&self) {
            *self.released.lock().unwrap() = true;
            self.changed.notify_all();
        }
    }

    impl AttentionAudioPort for BlockingAudio {
        fn play_ordinary(&self) -> Result<(), NativeEffectError> {
            self.calls.push("ordinary");
            Ok(())
        }

        fn stop_ordinary(&self) -> Result<(), NativeEffectError> {
            self.calls.push("ordinary-stop");
            Ok(())
        }

        fn start_timer_loop(
            &self,
            _key: AlarmPlaybackKey,
            _bytes: Arc<[u8]>,
        ) -> Result<(), NativeEffectError> {
            self.calls.push("timer-start");
            let released = self.released.lock().unwrap();
            drop(
                self.changed
                    .wait_while(released, |released| !*released)
                    .unwrap(),
            );
            if self.succeeds {
                Ok(())
            } else {
                self.calls.push("timer-fail");
                Err(NativeEffectError)
            }
        }

        fn stop_timer_loop(&self, _key: AlarmPlaybackKey) -> Result<(), NativeEffectError> {
            self.calls.push("timer-stop");
            Ok(())
        }

        fn shutdown(&self) {
            self.release();
            self.calls.push("audio-shutdown");
        }
    }

    #[derive(Default)]
    struct FakeTimers;

    impl TimerAttentionPort for FakeTimers {
        fn admit_audio_start(&self, _ticket: &AudioTicket) -> Result<bool, TimerError> {
            Ok(true)
        }

        fn confirm_audio_without_start(&self, _ticket: &AudioTicket) -> Result<bool, TimerError> {
            Ok(true)
        }

        fn confirm_audio_after_play_failure(
            &self,
            _ticket: &AudioTicket,
        ) -> Result<bool, TimerError> {
            Ok(true)
        }

        fn linearize_focused(&self, admission: &mut dyn FnMut()) -> Result<(), TimerError> {
            admission();
            Ok(())
        }

        fn terminate_all_audio(&self) -> Result<(), TimerError> {
            Ok(())
        }
    }

    struct OrderingTimers(Arc<Calls>);

    impl TimerAttentionPort for OrderingTimers {
        fn admit_audio_start(&self, _ticket: &AudioTicket) -> Result<bool, TimerError> {
            Ok(true)
        }

        fn confirm_audio_without_start(&self, _ticket: &AudioTicket) -> Result<bool, TimerError> {
            Ok(true)
        }

        fn confirm_audio_after_play_failure(
            &self,
            _ticket: &AudioTicket,
        ) -> Result<bool, TimerError> {
            Ok(true)
        }

        fn linearize_focused(&self, admission: &mut dyn FnMut()) -> Result<(), TimerError> {
            self.0.push("timer-lock");
            admission();
            self.0.push("timer-confirm");
            Ok(())
        }

        fn terminate_all_audio(&self) -> Result<(), TimerError> {
            Ok(())
        }
    }

    struct ScriptedTimers(Mutex<VecDeque<bool>>);

    impl TimerAttentionPort for ScriptedTimers {
        fn admit_audio_start(&self, _ticket: &AudioTicket) -> Result<bool, TimerError> {
            Ok(self.0.lock().unwrap().pop_front().unwrap_or(true))
        }

        fn confirm_audio_without_start(&self, _ticket: &AudioTicket) -> Result<bool, TimerError> {
            Ok(true)
        }

        fn confirm_audio_after_play_failure(
            &self,
            _ticket: &AudioTicket,
        ) -> Result<bool, TimerError> {
            Ok(true)
        }

        fn linearize_focused(&self, admission: &mut dyn FnMut()) -> Result<(), TimerError> {
            admission();
            Ok(())
        }

        fn terminate_all_audio(&self) -> Result<(), TimerError> {
            Ok(())
        }
    }

    struct NoopRoute;

    impl AttentionRoutePort for NoopRoute {
        fn show_messages(&self) {}
    }

    struct FakeRoute(Arc<Calls>);

    impl AttentionRoutePort for FakeRoute {
        fn show_messages(&self) {
            self.0.push("route");
        }
    }

    fn coordinator(calls: Arc<Calls>) -> Arc<NativeAttentionCoordinator> {
        NativeAttentionCoordinator::start_with_timer_port(
            Arc::new(FakeTimers),
            Arc::new(FakeToast(Arc::clone(&calls))),
            Arc::new(FakeTray(Arc::clone(&calls))),
            Arc::new(FakeAudio(calls)),
            Arc::new(NoopRoute),
        )
    }

    fn coordinator_with_route(calls: Arc<Calls>) -> Arc<NativeAttentionCoordinator> {
        NativeAttentionCoordinator::start_with_timer_port(
            Arc::new(FakeTimers),
            Arc::new(FakeToast(Arc::clone(&calls))),
            Arc::new(FakeTray(Arc::clone(&calls))),
            Arc::new(FakeAudio(Arc::clone(&calls))),
            Arc::new(FakeRoute(calls)),
        )
    }

    fn coordinator_with_ordering_timer(calls: Arc<Calls>) -> Arc<NativeAttentionCoordinator> {
        NativeAttentionCoordinator::start_with_timer_port(
            Arc::new(OrderingTimers(Arc::clone(&calls))),
            Arc::new(FakeToast(Arc::clone(&calls))),
            Arc::new(FakeTray(Arc::clone(&calls))),
            Arc::new(FakeAudio(calls)),
            Arc::new(NoopRoute),
        )
    }

    fn coordinator_with_ports(
        calls: Arc<Calls>,
        timers: Arc<dyn TimerAttentionPort>,
        audio: Arc<dyn AttentionAudioPort>,
    ) -> Arc<NativeAttentionCoordinator> {
        NativeAttentionCoordinator::start_with_timer_port(
            timers,
            Arc::new(FakeToast(Arc::clone(&calls))),
            Arc::new(FakeTray(calls)),
            audio,
            Arc::new(NoopRoute),
        )
    }

    fn ordinary(id: &str) -> PublishedAttention {
        PublishedAttention {
            message: MessagePublished {
                id: id.into(),
                plugin_id: "com.example.messages".into(),
                plugin_name_snapshot: "Messages".into(),
                created_at: "2026-08-21T00:00:00Z".into(),
                content: "hello".into(),
                revision: id.into(),
                unread_count: 1,
            },
            origin: AttentionOrigin::Ordinary,
        }
    }

    fn ticket(plugin_id: &str, audio_id: u64) -> AudioTicket {
        AudioTicket {
            key: TimerKey::new(plugin_id, 1, 1).unwrap(),
            round_id: 1,
            audio_id,
            fired_revision: audio_id,
        }
    }

    fn timer(id: &str, ticket: AudioTicket) -> PublishedAttention {
        let mut attention = ordinary(id);
        let alarm = ValidatedAlarmAsset {
            identity: AlarmAssetIdentity {
                plugin_id: ticket.key().plugin_id.clone(),
                plugin_generation: ticket.key().plugin_generation,
                activation_id: ticket.key().activation_id,
                package_digest: "package".into(),
                resource_sha256: "resource".into(),
                fixed_relative_path: "assets/sounds/timer-alarm.wav",
            },
            bytes: Arc::from([1_u8]),
        };
        attention.origin = AttentionOrigin::TimerCompletion {
            audio: Some(Box::new(TimerAudioCompletion { ticket, alarm })),
        };
        attention
    }

    fn timer_without_audio(id: &str) -> PublishedAttention {
        let mut attention = ordinary(id);
        attention.origin = AttentionOrigin::TimerCompletion { audio: None };
        attention
    }

    #[test]
    fn message_before_focus_runs_effects_then_stops_attention() {
        let calls = Arc::new(Calls::default());
        let coordinator = coordinator(Arc::clone(&calls));

        coordinator.publish(ordinary("1"));
        assert_eq!(calls.wait_for(3), ["toast", "tray", "ordinary"]);
        coordinator.observe_main_focus(true);
        assert_eq!(
            calls.wait_for(5),
            ["toast", "tray", "ordinary", "focus", "ordinary-stop"]
        );
        coordinator.shutdown();
    }

    #[test]
    fn focus_before_message_suppresses_all_native_message_effects() {
        let calls = Arc::new(Calls::default());
        let coordinator = coordinator(Arc::clone(&calls));

        coordinator.observe_main_focus(true);
        assert_eq!(calls.wait_for(1), ["focus"]);
        coordinator.publish(ordinary("1"));
        coordinator.observe_main_focus(false);
        coordinator.publish(ordinary("2"));
        assert_eq!(calls.wait_for(4), ["focus", "toast", "tray", "ordinary"]);
        coordinator.shutdown();
    }

    #[test]
    fn first_valid_timer_owns_the_epoch_and_later_tickets_never_start() {
        let calls = Arc::new(Calls::default());
        let coordinator = coordinator(Arc::clone(&calls));
        let first = ticket("com.example.timer.first", 1);
        let second = ticket("com.example.timer.second", 2);

        coordinator.publish(timer("1", first.clone()));
        coordinator.publish(timer("2", second.clone()));
        assert_eq!(calls.wait_for(4), ["toast", "tray", "timer-start", "toast"]);

        coordinator.publish(ordinary("3"));
        assert_eq!(
            calls.wait_for(5),
            ["toast", "tray", "timer-start", "toast", "toast"]
        );

        coordinator.cancel_timer_audio(first);
        assert_eq!(calls.wait_for(6).last(), Some(&"timer-stop"));
        coordinator.cancel_timer_audio(second);
        coordinator.shutdown();
    }

    #[test]
    fn focus_then_blur_does_not_confirm_a_later_timer_ticket() {
        let calls = Arc::new(Calls::default());
        let coordinator = coordinator(Arc::clone(&calls));

        coordinator.observe_main_focus(true);
        coordinator.observe_main_focus(false);
        assert_eq!(calls.wait_for(1), ["focus"]);
        coordinator.publish(timer("1", ticket("com.example.timer.after-focus", 1)));
        assert_eq!(calls.wait_for(4), ["focus", "toast", "tray", "timer-start"]);
        coordinator.shutdown();
    }

    #[test]
    fn toast_callback_keeps_only_the_first_terminal_outcome() {
        let calls = Arc::new(Calls::default());
        let coordinator = coordinator_with_route(Arc::clone(&calls));

        coordinator.publish(ordinary("1"));
        assert_eq!(calls.wait_for(3), ["toast", "tray", "ordinary"]);
        coordinator.toast_callback(1, ToastCallbackKind::Activated);
        assert_eq!(calls.wait_for(4).last(), Some(&"route"));
        coordinator.toast_callback(1, ToastCallbackKind::Failed);
        coordinator.toast_callback(1, ToastCallbackKind::Dismissed);
        assert_eq!(
            calls
                .wait_for(4)
                .iter()
                .filter(|call| **call == "route")
                .count(),
            1
        );
        coordinator.shutdown();
    }

    #[test]
    fn sequence_allocation_is_checked_and_never_wraps() {
        let mut state = MailboxState {
            next_sequence: u64::MAX - 1,
            ..MailboxState::default()
        };

        assert_eq!(state.allocate_sequence(), Some(u64::MAX));
        assert_eq!(state.allocate_sequence(), None);
        assert_eq!(state.next_sequence, u64::MAX);
    }

    #[test]
    fn focused_true_wakes_worker_only_after_timer_confirmation() {
        let calls = Arc::new(Calls::default());
        let coordinator = coordinator_with_ordering_timer(Arc::clone(&calls));

        coordinator.observe_main_focus(true);

        assert_eq!(calls.wait_for(3), ["timer-lock", "timer-confirm", "focus"]);
        coordinator.shutdown();
    }

    #[test]
    fn timer_completion_without_ticket_never_degrades_to_ordinary_audio() {
        let calls = Arc::new(Calls::default());
        let coordinator = coordinator(Arc::clone(&calls));

        coordinator.publish(timer_without_audio("1"));

        assert_eq!(calls.wait_for(2), ["toast", "tray"]);
        coordinator.shutdown();
    }

    #[test]
    fn invalid_first_ticket_does_not_claim_the_epoch_from_the_next_valid_ticket() {
        let calls = Arc::new(Calls::default());
        let timers: Arc<dyn TimerAttentionPort> =
            Arc::new(ScriptedTimers(Mutex::new(VecDeque::from([false, true]))));
        let audio: Arc<dyn AttentionAudioPort> = Arc::new(FakeAudio(Arc::clone(&calls)));
        let coordinator = coordinator_with_ports(Arc::clone(&calls), timers, audio);

        coordinator.publish(timer("1", ticket("com.example.invalid", 1)));
        coordinator.publish(timer("2", ticket("com.example.valid", 2)));

        assert_eq!(calls.wait_for(4), ["toast", "tray", "toast", "timer-start"]);
        coordinator.shutdown();
    }

    #[test]
    fn focus_during_start_revokes_the_owner_and_late_success_stops_only_its_key() {
        let calls = Arc::new(Calls::default());
        let audio = BlockingAudio::new(Arc::clone(&calls));
        let coordinator =
            coordinator_with_ports(Arc::clone(&calls), Arc::new(FakeTimers), audio.clone());

        coordinator.publish(timer("1", ticket("com.example.focus-race", 1)));
        assert_eq!(calls.wait_for(3), ["toast", "tray", "timer-start"]);
        coordinator.observe_main_focus(true);
        audio.release();

        assert_eq!(
            calls.wait_for(5),
            ["toast", "tray", "timer-start", "timer-stop", "focus"]
        );
        coordinator.shutdown();
    }

    #[test]
    fn matching_cancel_during_start_revokes_without_letting_an_old_stop_hit_a_new_owner() {
        let calls = Arc::new(Calls::default());
        let audio = BlockingAudio::new(Arc::clone(&calls));
        let coordinator =
            coordinator_with_ports(Arc::clone(&calls), Arc::new(FakeTimers), audio.clone());
        let owner = ticket("com.example.cancel-race", 1);

        coordinator.publish(timer("1", owner.clone()));
        assert_eq!(calls.wait_for(3), ["toast", "tray", "timer-start"]);
        coordinator.cancel_timer_audio(owner);
        audio.release();

        assert_eq!(calls.wait_for(4).last(), Some(&"timer-stop"));
        coordinator.shutdown();
    }

    #[test]
    fn failed_start_has_no_same_epoch_fallback_but_a_new_epoch_can_try() {
        let calls = Arc::new(Calls::default());
        let audio = BlockingAudio::new_failing(Arc::clone(&calls));
        let coordinator =
            coordinator_with_ports(Arc::clone(&calls), Arc::new(FakeTimers), audio.clone());

        coordinator.publish(timer("1", ticket("com.example.failed-owner", 1)));
        assert_eq!(calls.wait_for(3), ["toast", "tray", "timer-start"]);
        coordinator.publish(timer("2", ticket("com.example.same-epoch", 2)));
        audio.release();
        assert_eq!(
            calls.wait_for(5),
            ["toast", "tray", "timer-start", "timer-fail", "toast"]
        );

        coordinator.publish(timer("3", ticket("com.example.new-epoch", 3)));
        let values = calls.wait_for(8);
        assert_eq!(
            values
                .iter()
                .filter(|value| **value == "timer-start")
                .count(),
            2
        );
        coordinator.shutdown();
    }

    #[test]
    fn alarm_epoch_exhaustion_fails_only_timer_audio_admission_closed() {
        let mut mailbox = MailboxState::default();
        mailbox.alarm_epoch = u64::MAX;

        mailbox.advance_alarm_epoch();

        assert!(mailbox.timer_audio_terminal);
        assert!(mailbox.alarm_epoch_claimed);
        assert!(!mailbox.terminal);
    }
}
