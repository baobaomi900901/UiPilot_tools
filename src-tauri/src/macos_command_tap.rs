//! Optional passive session listener for a double press of one Command key.
//! Modifier flags drive the gesture. Ordinary keys and mouse buttons cancel it while held.
//! Key identities, typed text and pointer coordinates are never read.
use std::{
    cell::RefCell,
    ffi::c_void,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc},
    thread::{self, JoinHandle},
    time::{Duration, SystemTime},
};

use core_foundation::{
    base::TCFType,
    mach_port::{CFMachPort, CFMachPortInvalidate, CFMachPortRef},
    runloop::{kCFRunLoopDefaultMode, CFRunLoop},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StartError { InputMonitoringRequired, Unavailable }

const WINDOW: Duration = Duration::from_millis(400);
// IOHID device-dependent Command bits distinguish the two physical keys.
const LEFT: u64 = 0x08;
const RIGHT: u64 = 0x10;
const COMMAND: u64 = 1 << 20;
const OTHER_MODIFIERS: u64 = (1 << 17) | (1 << 18) | (1 << 19) | (1 << 22) | (1 << 23);
const LEFT_MOUSE_DOWN: u32 = 1;
const LEFT_MOUSE_UP: u32 = 2;
const RIGHT_MOUSE_DOWN: u32 = 3;
const RIGHT_MOUSE_UP: u32 = 4;
const OTHER_MOUSE_DOWN: u32 = 25;
const OTHER_MOUSE_UP: u32 = 26;
const KEY_DOWN: u32 = 10;
const KEY_UP: u32 = 11;
const AUTOREPEAT_FIELD: u32 = 8;
const FLAGS_CHANGED: u32 = 12;
const DISABLED_TIMEOUT: u32 = 0xffff_fffe;
const DISABLED_USER_INPUT: u32 = 0xffff_ffff;

#[derive(Clone, Copy)]
struct Stamp { event_ns: u64, wall: SystemTime }

impl Stamp {
    fn within(self, now: Self) -> bool {
        now.event_ns.checked_sub(self.event_ns).is_some_and(|ns| Duration::from_nanos(ns) <= WINDOW)
            && now.wall.duration_since(self.wall).is_ok_and(|elapsed| elapsed <= WINDOW)
    }
}

#[derive(Default)]
enum Gesture {
    #[default]
    Idle,
    Down { side: u64, start: Stamp, second: bool },
    Between { side: u64, start: Stamp },
    Blocked,
}

impl Gesture {
    fn reset(&mut self, flags: u64) {
        *self = if flags & (COMMAND | LEFT | RIGHT | OTHER_MODIFIERS) == 0 {
            Self::Idle
        } else {
            Self::Blocked
        };
    }

    fn cancel(&mut self) {
        *self = match self {
            Self::Down { .. } | Self::Blocked => Self::Blocked,
            _ => Self::Idle,
        };
    }

    fn flags(&mut self, flags: u64, now: Stamp) -> bool {
        let side = flags & (LEFT | RIGHT);
        if flags & (COMMAND | LEFT | RIGHT | OTHER_MODIFIERS) == 0 {
            let previous = std::mem::take(self);
            if let Self::Down { side, start, second } = previous {
                if start.within(now) {
                    if second { return true; }
                    *self = Self::Between { side, start };
                }
            }
        } else if flags & OTHER_MODIFIERS == 0 && flags & COMMAND != 0 && matches!(side, LEFT | RIGHT) {
            *self = match *self {
                Self::Idle => Self::Down { side, start: now, second: false },
                Self::Between { side: first, start } if first == side && start.within(now) => {
                    Self::Down { side, start, second: true }
                }
                Self::Between { .. } => Self::Down { side, start: now, second: false },
                Self::Blocked | Self::Down { .. } => Self::Blocked,
            };
        } else {
            *self = Self::Blocked;
        }
        false
    }
}

type Event = *mut c_void;
type Callback = unsafe extern "C" fn(*mut c_void, u32, Event, *mut c_void) -> Event;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
    fn CGEventTapCreate(location: u32, placement: u32, options: u32, mask: u64,
        callback: Callback, info: *mut c_void) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventTapIsEnabled(tap: CFMachPortRef) -> bool;
    fn CGEventGetFlags(event: Event) -> u64;
    fn CGEventGetTimestamp(event: Event) -> u64;
    fn CGEventGetIntegerValueField(event: Event, field: u32) -> i64;
    fn CGEventSourceFlagsState(state: i32) -> u64;
}

fn has_permission() -> bool {
    // Preflight never asks for permission.
    unsafe { CGPreflightListenEventAccess() }
}

fn permission(allowed: bool, request: bool, ask: impl FnOnce() -> bool) -> Result<(), StartError> {
    if allowed || (request && ask()) { Ok(()) } else { Err(StartError::InputMonitoringRequired) }
}

#[derive(Default)]
struct InputState {
    gesture: Gesture,
    held_keys: usize,
    held_buttons: usize,
}

impl InputState {
    fn key_down(&mut self, repeat: bool) {
        // A repeat seen immediately after registration also proves a key is held.
        self.held_keys = if repeat { self.held_keys.max(1) } else { self.held_keys.saturating_add(1) };
        self.gesture.cancel();
    }

    fn mouse_down(&mut self) {
        self.held_buttons = self.held_buttons.saturating_add(1);
        self.gesture.cancel();
    }

    fn mouse_up(&mut self) {
        self.held_buttons = self.held_buttons.saturating_sub(1);
        self.gesture.cancel();
    }

    fn key_up(&mut self) {
        self.held_keys = self.held_keys.saturating_sub(1);
        self.gesture.cancel();
    }

    fn flags(&mut self, flags: u64, now: Stamp) -> bool {
        if self.held_keys != 0 || self.held_buttons != 0 {
            self.gesture.reset(flags);
            return false;
        }
        self.gesture.flags(flags, now)
    }

    fn synchronize(&mut self, flags: u64) {
        // Transitions may have been lost while disabled, including releases.
        self.held_keys = 0;
        self.held_buttons = 0;
        self.gesture.reset(flags);
    }
}

struct Listener {
    input: InputState,
    active: Arc<AtomicBool>,
    on_double: Box<dyn Fn() + Send>,
}

impl Listener {
    fn synchronize(&mut self) {
        // kCGEventSourceStateCombinedSessionState. Read aggregate modifiers only.
        self.input.synchronize(unsafe { CGEventSourceFlagsState(0) });
    }
}

// SAFETY: The context remains at a stable address on the worker until its run loop
// has removed the source and invalidated the tap. Only that worker runs callbacks.
unsafe extern "C" fn observe(_: *mut c_void, kind: u32, event: Event, info: *mut c_void) -> Event {
    let context = unsafe { &*info.cast::<RefCell<Listener>>() };
    let Ok(mut state) = context.try_borrow_mut() else { return event; };
    if !state.active.load(Ordering::Acquire) { return event; }
    match kind {
        DISABLED_TIMEOUT | DISABLED_USER_INPUT => state.synchronize(),
        KEY_DOWN if !event.is_null() => {
            // Read only the repeat bit, never virtual keycode, characters or text.
            state.input.key_down(unsafe { CGEventGetIntegerValueField(event, AUTOREPEAT_FIELD) } != 0);
        }
        KEY_UP => state.input.key_up(),
        LEFT_MOUSE_DOWN | RIGHT_MOUSE_DOWN | OTHER_MOUSE_DOWN => state.input.mouse_down(),
        LEFT_MOUSE_UP | RIGHT_MOUSE_UP | OTHER_MOUSE_UP => state.input.mouse_up(),
        FLAGS_CHANGED if !event.is_null() => {
            let now = Stamp { event_ns: unsafe { CGEventGetTimestamp(event) }, wall: SystemTime::now() };
            if state.input.flags(unsafe { CGEventGetFlags(event) }, now) {
                if !has_permission() || catch_unwind(AssertUnwindSafe(|| (state.on_double)())).is_err() {
                    state.active.store(false, Ordering::Release);
                }
            }
        }
        _ => {}
    }
    event
}

pub(crate) struct CommandTap {
    active: Arc<AtomicBool>,
    runloop: CFRunLoop,
    worker: Option<JoinHandle<()>>,
}

impl CommandTap {
    /// Use request_permission only for an explicit settings action. The callback
    /// runs on the listener thread and should enqueue UI work without waiting.
    pub(crate) fn start(request_permission: bool, on_double: impl Fn() + Send + 'static) -> Result<Self, StartError> {
        permission(has_permission(), request_permission, || unsafe { CGRequestListenEventAccess() })?;
        let active = Arc::new(AtomicBool::new(true));
        let (ready, status) = mpsc::sync_channel(1);
        let listener = Listener { input: InputState::default(), active: Arc::clone(&active), on_double: Box::new(on_double) };
        let worker = thread::Builder::new().name("uipilot-command-tap".into()).spawn(move || {
            run(listener, ready);
        }).map_err(|_| StartError::Unavailable)?;
        match status.recv().unwrap_or(Err(StartError::Unavailable)) {
            Ok(runloop) => Ok(Self { active, runloop, worker: Some(worker) }),
            Err(error) => { let _ = worker.join(); Err(error) }
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire) && has_permission()
    }
}

impl Drop for CommandTap {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        self.runloop.stop();
        if let Some(worker) = self.worker.take() {
            if worker.thread().id() != thread::current().id() { let _ = worker.join(); }
        }
    }
}

fn unavailable() -> StartError {
    if has_permission() { StartError::Unavailable } else { StartError::InputMonitoringRequired }
}

fn open_tap(context: &RefCell<Listener>) -> Result<CFMachPort, StartError> {
    const SESSION: u32 = 1;
    const TAIL: u32 = 1;
    const LISTEN_ONLY: u32 = 1;
    let mask = [
        KEY_DOWN, KEY_UP, FLAGS_CHANGED, LEFT_MOUSE_DOWN, LEFT_MOUSE_UP,
        RIGHT_MOUSE_DOWN, RIGHT_MOUSE_UP, OTHER_MOUSE_DOWN, OTHER_MOUSE_UP,
    ].into_iter().fold(0u64, |mask, kind| mask | (1u64 << kind));
    let info = std::ptr::from_ref(context).cast_mut().cast();
    let port = unsafe { CGEventTapCreate(SESSION, TAIL, LISTEN_ONLY, mask, observe, info) };
    if port.is_null() { return Err(unavailable()); }
    // Create rule: the worker owns this port; callbacks run only after scheduling it.
    Ok(unsafe { CFMachPort::wrap_under_create_rule(port) })
}

fn run(listener: Listener, ready: mpsc::SyncSender<Result<CFRunLoop, StartError>>) {
    let active = Arc::clone(&listener.active);
    let context = RefCell::new(listener);
    let runloop = CFRunLoop::get_current();
    let mode = unsafe { kCFRunLoopDefaultMode };
    let mut startup = Some(ready);
    while active.load(Ordering::Acquire) {
        if !has_permission() { break; }
        let Ok(tap) = open_tap(&context) else { break; };
        let port = tap.as_concrete_TypeRef();
        let Ok(source) = tap.create_runloop_source(0) else {
            unsafe { CFMachPortInvalidate(port) };
            break;
        };
        context.borrow_mut().synchronize();
        runloop.add_source(&source, mode);
        unsafe { CGEventTapEnable(port, true) };
        let enabled = unsafe { CGEventTapIsEnabled(port) };
        if enabled {
            if let Some(sender) = startup.take() {
                if sender.send(Ok(runloop.clone())).is_err() {
                    active.store(false, Ordering::Release);
                }
            }
            while active.load(Ordering::Acquire) {
                CFRunLoop::run_in_mode(mode, Duration::from_secs(1), false);
                if !has_permission() {
                    active.store(false, Ordering::Release);
                    break;
                }
                if !unsafe { CGEventTapIsEnabled(port) } {
                    context.borrow_mut().synchronize();
                    unsafe { CGEventTapEnable(port, true) };
                    // WindowServer can invalidate a port during sleep. Recreate it
                    // on this run loop without requesting fresh permission.
                    if !unsafe { CGEventTapIsEnabled(port) } { break; }
                }
            }
        }
        runloop.remove_source(&source, mode);
        unsafe { CFMachPortInvalidate(port) };
        if !enabled { break; }
    }
    active.store(false, Ordering::Release);
    if let Some(sender) = startup { let _ = sender.send(Err(unavailable())); }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(ms: u64) -> Stamp {
        Stamp { event_ns: ms * 1_000_000, wall: SystemTime::UNIX_EPOCH + Duration::from_millis(ms) }
    }

    fn fired(events: &[(u64, u64)]) -> Vec<usize> {
        let mut detector = Gesture::default();
        events.iter().enumerate().filter_map(|(index, &(ms, flags))| {
            detector.flags(flags, stamp(ms)).then_some(index)
        }).collect()
    }

    #[test]
    fn each_side_requires_two_complete_taps_within_400ms() {
        for cmd in [COMMAND | LEFT, COMMAND | RIGHT] {
            assert_eq!(fired(&[(0, cmd), (120, 0), (260, cmd), (400, 0)]), [3]);
            assert!(fired(&[(0, cmd), (120, 0), (260, cmd), (401, 0)]).is_empty());
            assert!(fired(&[(0, cmd), (500, 0), (520, cmd), (530, 0)]).is_empty());
        }
    }

    #[test]
    fn mixed_modifiers_duplicate_transitions_and_ordinary_keys_cancel() {
        let left = COMMAND | LEFT;
        let right = COMMAND | RIGHT;
        for sequence in [
            vec![left, 0, right, 0],
            vec![left, left | RIGHT, right, 0, right, 0],
            vec![left, left, 0, left, 0],
            vec![left, 0, 0, left, 0],
            vec![left, left | (1 << 17), left, 0, left, 0],
            vec![COMMAND, 0, left, 0],
        ] {
            let events: Vec<_> = sequence.into_iter().enumerate().map(|(i, f)| (i as u64, f)).collect();
            assert!(fired(&events).is_empty());
        }
        for cancel_at in 0..3 {
            let mut detector = Gesture::default();
            for (i, flags) in [left, 0, left, 0].into_iter().enumerate() {
                assert!(!detector.flags(flags, stamp(i as u64)));
                if i == cancel_at { detector.cancel(); }
            }
        }
    }

    #[test]
    fn held_ordinary_keys_and_repeats_block_until_all_keys_are_released() {
        let cmd = COMMAND | RIGHT;
        let mut input = InputState::default();
        input.key_down(false);
        input.key_down(false);
        input.key_down(true);
        input.key_down(true);
        for (i, f) in [cmd, 0, cmd, 0].into_iter().enumerate() {
            assert!(!input.flags(f, stamp(i as u64)));
        }
        input.key_up();
        assert!(!input.flags(cmd, stamp(4)));
        input.key_up(); // The Command already pressed in the chord cannot count.
        assert!(!input.flags(0, stamp(5)));
        for (i, f) in [cmd, 0, cmd, 0].into_iter().enumerate() {
            assert_eq!(input.flags(f, stamp(10 + i as u64)), i == 3);
        }
        // Attaching mid-repeat still blocks until release; missing releases at
        // re-enable cannot leave the detector stuck forever.
        input.key_down(true);
        assert_eq!(input.held_keys, 1);
        input.synchronize(0);
        assert_eq!(input.held_keys, 0);
    }

    #[test]
    fn command_click_and_click_between_taps_cancel_the_pending_gesture() {
        let cmd = COMMAND | LEFT;
        for click_after in [0, 1, 2] {
            let mut input = InputState::default();
            for (index, flags) in [cmd, 0, cmd, 0].into_iter().enumerate() {
                assert!(!input.flags(flags, stamp(index as u64)));
                if index == click_after { input.mouse_down(); input.mouse_up(); }
            }
        }
    }

    #[test]
    fn holding_mouse_buttons_suppresses_command_until_the_last_release() {
        let cmd = COMMAND | RIGHT;
        let mut input = InputState::default();
        input.mouse_down();
        input.mouse_down();
        input.mouse_up();
        for (index, flags) in [cmd, 0, cmd, 0].into_iter().enumerate() {
            assert!(!input.flags(flags, stamp(index as u64)));
        }
        assert!(!input.flags(cmd, stamp(8)));
        input.mouse_up();
        assert!(!input.flags(0, stamp(9))); // Release of a chord cannot count as the first tap.
        for (index, flags) in [cmd, 0, cmd, 0].into_iter().enumerate() {
            assert_eq!(input.flags(flags, stamp(10 + index as u64)), index == 3);
        }
        input.mouse_down();
        input.synchronize(0);
        assert_eq!(input.held_buttons, 0);
    }

    #[test]
    fn resets_clock_jumps_and_sleep_discard_incomplete_pairs() {
        let cmd = COMMAND | LEFT;
        for synchronization in [0, cmd] {
            let mut detector = Gesture::default();
            detector.flags(cmd, stamp(0));
            detector.flags(0, stamp(10));
            detector.reset(synchronization);
            assert!(!detector.flags(cmd, stamp(20)));
            assert!(!detector.flags(0, stamp(30)));
        }
        for wall in [stamp(0).wall, stamp(60_000).wall] {
            let mut detector = Gesture::default();
            detector.flags(cmd, stamp(50));
            detector.flags(0, stamp(60));
            detector.flags(cmd, stamp(70));
            assert!(!detector.flags(0, Stamp { wall, ..stamp(80) }));
        }
    }

    #[test]
    fn restoring_shortcut_never_requests_input_monitoring() {
        assert_eq!(permission(false, false, || panic!("startup prompt")), Err(StartError::InputMonitoringRequired));
        assert_eq!(permission(true, true, || panic!("already granted")), Ok(()));
        assert_eq!(permission(false, true, || false), Err(StartError::InputMonitoringRequired));
        assert_eq!(permission(false, true, || true), Ok(()));
    }
}
