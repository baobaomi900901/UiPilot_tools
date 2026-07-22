use std::{
    sync::{
        atomic::{AtomicU64, AtomicU8, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{WM_ENDSESSION, WM_NCDESTROY, WM_QUERYENDSESSION},
    },
};

use crate::{
    commands::clear_and_hide,
    file_index::FileIndex,
    hotkey::{DoubleTapModifier, HotkeyKind},
    hotkey_hook::HotkeyHook,
    result_registry::ResultRegistry,
    settings::{Settings, SettingsStore, SettingsUpdate},
    validation_data::{ValidationEvent, ValidationStore},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ShowTarget {
    Launcher,
    Settings,
}

pub(crate) const TRAY_OPEN_LAUNCHER: &str = "uipilot.tray.open-launcher";
pub(crate) const TRAY_OPEN_SETTINGS: &str = "uipilot.tray.open-settings";
pub(crate) const TRAY_QUIT: &str = "uipilot.tray.quit";
const SESSION_SUBCLASS_ID: usize = 0x5550_494c;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TrayAction {
    Show(ShowTarget),
    Quit,
}

pub(crate) fn tray_action(id: &str) -> Option<TrayAction> {
    match id {
        TRAY_OPEN_LAUNCHER => Some(TrayAction::Show(ShowTarget::Launcher)),
        TRAY_OPEN_SETTINGS => Some(TrayAction::Show(ShowTarget::Settings)),
        TRAY_QUIT => Some(TrayAction::Quit),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum LifecycleNotice {
    SettingsFailed,
    ValidationFailed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct LauncherShown {
    invocation_id: String,
    target: ShowTarget,
    notice: Option<LifecycleNotice>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LifecycleError {
    MainThreadDispatchFailed,
    WindowFailed,
    InvocationExhausted,
    SessionHookFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShowOutcome {
    Shown,
    Ignored,
}

#[derive(Debug, Default)]
struct Readiness {
    setup_ready: bool,
    frontend_ready: bool,
    pending_target: Option<ShowTarget>,
}

impl Readiness {
    fn request(&mut self, target: ShowTarget) -> Option<ShowTarget> {
        if self.setup_ready && self.frontend_ready {
            Some(target)
        } else {
            self.pending_target = Some(target);
            None
        }
    }

    fn mark_setup_ready(&mut self) -> Option<ShowTarget> {
        self.setup_ready = true;
        self.take_if_ready()
    }

    fn mark_frontend_ready(&mut self) -> Option<ShowTarget> {
        self.frontend_ready = true;
        self.take_if_ready()
    }

    fn take_if_ready(&mut self) -> Option<ShowTarget> {
        if self.setup_ready && self.frontend_ready {
            self.pending_target.take()
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, Default)]
struct RuntimeSettings {
    registered: Vec<Shortcut>,
    installed_hook: Option<DoubleTapModifier>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct HotkeyBindingChange {
    persisted: HotkeyKind,
    requested: HotkeyKind,
    autostart: bool,
}

impl RuntimeSettings {
    fn apply_hotkey_binding<R, U, IH, UH, A, C, P>(
        &mut self,
        change: HotkeyBindingChange,
        shortcut_ops: (R, U),
        hook_ops: (IH, UH),
        autostart_ops: (A, C),
        persist: P,
    ) -> Result<(), ()>
    where
        R: FnMut(Shortcut) -> Result<(), ()>,
        U: FnMut(Shortcut) -> Result<(), ()>,
        IH: FnMut(DoubleTapModifier) -> Result<(), ()>,
        UH: FnMut() -> Result<(), ()>,
        A: FnOnce() -> Result<bool, ()>,
        C: FnMut(bool) -> Result<(), ()>,
        P: FnOnce() -> Result<(), ()>,
    {
        let (mut register_shortcut, mut unregister_shortcut) = shortcut_ops;
        let (mut install_hook, mut uninstall_hook) = hook_ops;
        let (read_autostart, mut change_autostart) = autostart_ops;
        let before = self.clone();
        let mut previous_autostart = None;
        let mut changed_autostart = false;
        let result = (|| {
            match change.requested {
                HotkeyKind::DoubleTap(modifier) => {
                    if self.installed_hook != Some(modifier) {
                        if self.installed_hook.is_some() {
                            uninstall_hook()?;
                            self.installed_hook = None;
                        }
                        install_hook(modifier)?;
                        self.installed_hook = Some(modifier);
                    }
                    for shortcut in self.registered.clone() {
                        unregister_shortcut(shortcut)?;
                        self.registered.retain(|registered| *registered != shortcut);
                    }
                }
                HotkeyKind::Chord(requested) => {
                    let persisted = match change.persisted {
                        HotkeyKind::Chord(shortcut) => shortcut,
                        HotkeyKind::DoubleTap(_) => requested,
                    };
                    while self.registered.len() >= 2 && !self.registered.contains(&requested) {
                        let stale = self
                            .registered
                            .iter()
                            .copied()
                            .find(|registered| *registered != persisted)
                            .ok_or(())?;
                        unregister_shortcut(stale)?;
                        self.registered.retain(|registered| *registered != stale);
                    }
                    if !self.registered.contains(&requested) {
                        register_shortcut(requested)?;
                        self.registered.push(requested);
                    }
                    if self.installed_hook.is_some() {
                        uninstall_hook()?;
                        self.installed_hook = None;
                    }
                    for shortcut in self.registered.clone() {
                        if shortcut != requested {
                            unregister_shortcut(shortcut)?;
                            self.registered.retain(|registered| *registered != shortcut);
                        }
                    }
                }
            }

            let actual_autostart = read_autostart()?;
            previous_autostart = Some(actual_autostart);
            if actual_autostart != change.autostart {
                change_autostart(change.autostart)?;
                changed_autostart = true;
            }
            persist()
        })();

        if result.is_ok() {
            return Ok(());
        }

        if changed_autostart {
            let _ = change_autostart(previous_autostart.expect("changed autostart has old value"));
        }
        let _ = self.restore_hotkey_snapshot(
            &before,
            &mut register_shortcut,
            &mut unregister_shortcut,
            &mut install_hook,
            &mut uninstall_hook,
        );
        Err(())
    }

    fn restore_hotkey_snapshot<R, U, IH, UH>(
        &mut self,
        before: &Self,
        register_shortcut: &mut R,
        unregister_shortcut: &mut U,
        install_hook: &mut IH,
        uninstall_hook: &mut UH,
    ) -> Result<(), ()>
    where
        R: FnMut(Shortcut) -> Result<(), ()>,
        U: FnMut(Shortcut) -> Result<(), ()>,
        IH: FnMut(DoubleTapModifier) -> Result<(), ()>,
        UH: FnMut() -> Result<(), ()>,
    {
        let mut failed = false;
        for shortcut in &before.registered {
            while self.registered.len() >= 2 && !self.registered.contains(shortcut) {
                let Some(extra) = self
                    .registered
                    .iter()
                    .copied()
                    .find(|registered| !before.registered.contains(registered))
                else {
                    failed = true;
                    break;
                };
                if unregister_shortcut(extra).is_ok() {
                    self.registered.retain(|registered| *registered != extra);
                } else {
                    failed = true;
                    break;
                }
            }
            if !self.registered.contains(shortcut) {
                if register_shortcut(*shortcut).is_ok() {
                    self.registered.push(*shortcut);
                } else {
                    failed = true;
                }
            }
        }

        if self.installed_hook != before.installed_hook {
            if self.installed_hook.is_some() {
                if uninstall_hook().is_ok() {
                    self.installed_hook = None;
                } else {
                    failed = true;
                }
            }
            if self.installed_hook.is_none() {
                if let Some(modifier) = before.installed_hook {
                    if install_hook(modifier).is_ok() {
                        self.installed_hook = Some(modifier);
                    } else {
                        failed = true;
                    }
                }
            }
        }

        let previous_restored = before
            .registered
            .iter()
            .all(|shortcut| self.registered.contains(shortcut))
            && self.installed_hook == before.installed_hook;
        for shortcut in self.registered.clone() {
            if !before.registered.contains(&shortcut) {
                if unregister_shortcut(shortcut).is_ok() {
                    self.registered.retain(|registered| *registered != shortcut);
                } else {
                    failed = true;
                }
            }
        }

        if !failed && self.registered.len() == before.registered.len() && previous_restored {
            self.registered = before.registered.clone();
            Ok(())
        } else {
            Err(())
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ModalState {
    Normal,
    Open,
    AwaitingFocusRestore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FocusDecision {
    Suppress,
    ClearAndHide,
    ReportWindowFailureAndHide,
}

impl ModalState {
    fn finish_export(&mut self) {
        if *self == Self::Open {
            *self = Self::AwaitingFocusRestore;
        }
    }

    fn claim_export(&mut self) -> Result<bool, FocusDecision> {
        match self {
            Self::Normal => {
                *self = Self::Open;
                Ok(false)
            }
            Self::Open => Err(FocusDecision::Suppress),
            Self::AwaitingFocusRestore => {
                *self = Self::Open;
                Ok(true)
            }
        }
    }

    fn resolve_export_focus(&mut self, focused: Result<bool, ()>) -> Result<(), FocusDecision> {
        match focused {
            Ok(true) => Ok(()),
            Ok(false) => {
                *self = Self::Normal;
                Err(FocusDecision::ClearAndHide)
            }
            Err(()) => {
                *self = Self::Normal;
                Err(FocusDecision::ReportWindowFailureAndHide)
            }
        }
    }

    #[cfg(test)]
    fn on_focus<F>(&mut self, focused: bool, query_focus: F) -> FocusDecision
    where
        F: FnOnce() -> Result<bool, ()>,
    {
        if let Some(decision) = self.begin_focus(focused) {
            return decision;
        }
        match query_focus() {
            Ok(true) => FocusDecision::Suppress,
            Ok(false) => FocusDecision::ClearAndHide,
            Err(()) => FocusDecision::ReportWindowFailureAndHide,
        }
    }

    fn begin_focus(&mut self, focused: bool) -> Option<FocusDecision> {
        match (*self, focused) {
            (Self::Open, _) => Some(FocusDecision::Suppress),
            (Self::AwaitingFocusRestore, true) => {
                *self = Self::Normal;
                Some(FocusDecision::Suppress)
            }
            (Self::AwaitingFocusRestore, false) => {
                *self = Self::Normal;
                None
            }
            (Self::Normal, true) => Some(FocusDecision::Suppress),
            (Self::Normal, false) => Some(FocusDecision::ClearAndHide),
        }
    }

    fn on_successful_show(&mut self) {
        if *self == Self::AwaitingFocusRestore {
            *self = Self::Normal;
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExitState {
    Running,
    Cleaning,
    Clean,
    SystemEnding,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum FileIndexPhase {
    Running = 0,
    Cleaning = 1,
    Terminal = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CleanOwner {
    Tray,
    System,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CleanResult {
    Succeeded,
    Failed,
    TimedOut,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CleanAttempt {
    Idle,
    Waiting {
        owner: CleanOwner,
        deadline: Instant,
    },
    Calling {
        owner: CleanOwner,
        deadline: Instant,
    },
    Finished(CleanResult),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CleanDecision {
    Wait { deadline: Instant },
    CallMarker,
    Exit,
    ReturnRunning,
    ObserveOnly,
}

struct TrayCleanStart {
    decision: CleanDecision,
    attempt_epoch: u64,
    attempt_overflowed: bool,
    deadline: Instant,
}

#[derive(Debug)]
struct ExitGate {
    state: ExitState,
    in_flight_critical: usize,
    clean_attempt: CleanAttempt,
}

impl Default for ExitGate {
    fn default() -> Self {
        Self {
            state: ExitState::Running,
            in_flight_critical: 0,
            clean_attempt: CleanAttempt::Idle,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReservationError {
    NotRunning,
    Overflow,
}

#[derive(Debug)]
pub(crate) struct LifecycleCoordinator {
    next_invocation: AtomicU64,
    lifecycle_phase: AtomicU8,
    lifecycle_attempt_epoch: AtomicU64,
    readiness: Mutex<Readiness>,
    modal: Mutex<ModalState>,
    exit_gate: Mutex<ExitGate>,
    critical_changed: Condvar,
    pending_notice: Mutex<Option<LifecycleNotice>>,
    runtime_settings: Mutex<RuntimeSettings>,
    hotkey_hook: Mutex<Option<HotkeyHook>>,
}

impl Default for LifecycleCoordinator {
    fn default() -> Self {
        Self {
            next_invocation: AtomicU64::new(0),
            lifecycle_phase: AtomicU8::new(FileIndexPhase::Running as u8),
            lifecycle_attempt_epoch: AtomicU64::new(0),
            readiness: Mutex::new(Readiness::default()),
            modal: Mutex::new(ModalState::Normal),
            exit_gate: Mutex::new(ExitGate::default()),
            critical_changed: Condvar::new(),
            pending_notice: Mutex::new(None),
            runtime_settings: Mutex::new(RuntimeSettings::default()),
            hotkey_hook: Mutex::new(None),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ModalGuard {
    coordinator: Arc<LifecycleCoordinator>,
}

struct ModalRecoveryClaim {
    coordinator: Arc<LifecycleCoordinator>,
    committed: bool,
}

impl ModalRecoveryClaim {
    fn new(coordinator: &Arc<LifecycleCoordinator>) -> Self {
        Self {
            coordinator: Arc::clone(coordinator),
            committed: false,
        }
    }

    fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for ModalRecoveryClaim {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let mut modal = self.coordinator.modal.lock().expect("modal lock poisoned");
        if *modal == ModalState::Open {
            *modal = ModalState::Normal;
        }
    }
}

impl Drop for ModalGuard {
    fn drop(&mut self) {
        self.coordinator
            .modal
            .lock()
            .expect("modal lock poisoned")
            .finish_export();
    }
}

struct ShowMainClosures<'a> {
    center: &'a mut dyn FnMut() -> Result<(), ()>,
    always_on_top: &'a mut dyn FnMut() -> Result<(), ()>,
    show: &'a mut dyn FnMut() -> Result<(), ()>,
    focus: &'a mut dyn FnMut() -> Result<(), ()>,
    registry_on_show: &'a mut dyn FnMut(String),
    emit: &'a mut dyn FnMut(&LauncherShown) -> Result<(), ()>,
    clear_and_hide: &'a mut dyn FnMut(),
    record_launcher: &'a mut dyn FnMut(CriticalReservation),
}

type MainThreadOperation = Box<dyn FnOnce() + Send>;

fn dispatch_and_wait<D, O>(dispatch: D, operation: O) -> Result<(), ()>
where
    D: FnOnce(MainThreadOperation) -> Result<(), ()>,
    O: FnOnce() -> Result<(), ()> + Send + 'static,
{
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    dispatch(Box::new(move || {
        let _ = sender.send(operation());
    }))?;
    receiver.recv().map_err(|_| ())?
}

fn uninstall_slot_with<T, U>(slot: &Mutex<Option<T>>, uninstall: U) -> Result<(), ()>
where
    U: FnOnce(T) -> Result<(), T>,
{
    let mut slot = slot.lock().map_err(|_| ())?;
    let Some(installed) = slot.take() else {
        return Ok(());
    };
    match uninstall(installed) {
        Ok(()) => Ok(()),
        Err(installed) => {
            *slot = Some(installed);
            Err(())
        }
    }
}

#[derive(Debug)]
pub(crate) struct CriticalReservation {
    coordinator: Arc<LifecycleCoordinator>,
    released: bool,
}

impl Drop for CriticalReservation {
    fn drop(&mut self) {
        if self.released {
            return;
        }

        {
            let mut gate = self
                .coordinator
                .exit_gate
                .lock()
                .expect("exit gate lock poisoned");
            gate.in_flight_critical = gate
                .in_flight_critical
                .checked_sub(1)
                .expect("critical reservation count underflow");
        }
        self.released = true;
        self.coordinator.critical_changed.notify_all();
    }
}

impl LifecycleCoordinator {
    pub(crate) fn file_index_phase(&self) -> FileIndexPhase {
        match self.lifecycle_phase.load(Ordering::Acquire) {
            0 => FileIndexPhase::Running,
            1 => FileIndexPhase::Cleaning,
            _ => FileIndexPhase::Terminal,
        }
    }

    pub(crate) fn file_index_attempt_epoch(&self) -> u64 {
        self.lifecycle_attempt_epoch.load(Ordering::Acquire)
    }

    fn store_file_index_phase(&self, phase: FileIndexPhase) {
        self.lifecycle_phase.store(phase as u8, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn set_file_index_mirror_for_test(&self, phase: FileIndexPhase, attempt_epoch: u64) {
        self.lifecycle_attempt_epoch
            .store(attempt_epoch, Ordering::Release);
        self.store_file_index_phase(phase);
    }

    pub(crate) fn save_settings_transaction(
        self: &Arc<Self>,
        app: &AppHandle,
        settings: &SettingsStore,
        cache: &crate::apps::AppCache,
        kind: HotkeyKind,
        update: SettingsUpdate,
    ) -> Result<(), ()> {
        let persisted = settings.snapshot();
        let persisted_kind = HotkeyKind::parse(&persisted.hotkey).map_err(|_| ())?;
        self.apply_production_hotkey_transaction(
            app,
            HotkeyBindingChange {
                persisted: persisted_kind,
                requested: kind,
                autostart: update.autostart,
            },
            || settings.update_user_settings(update, cache).map_err(|_| ()),
        )
    }

    pub(crate) fn save_hotkey_transaction(
        self: &Arc<Self>,
        app: &AppHandle,
        settings: &SettingsStore,
        kind: HotkeyKind,
        hotkey: String,
    ) -> Result<(), ()> {
        let persisted = settings.snapshot();
        let persisted_kind = HotkeyKind::parse(&persisted.hotkey).map_err(|_| ())?;
        self.apply_production_hotkey_transaction(
            app,
            HotkeyBindingChange {
                persisted: persisted_kind,
                requested: kind,
                autostart: persisted.autostart,
            },
            || settings.update_hotkey(hotkey).map_err(|_| ()),
        )
    }

    fn apply_production_hotkey_transaction<P>(
        self: &Arc<Self>,
        app: &AppHandle,
        change: HotkeyBindingChange,
        persist: P,
    ) -> Result<(), ()>
    where
        P: FnOnce() -> Result<(), ()>,
    {
        let global_shortcut = app.global_shortcut();
        let autostart = app.autolaunch();
        let install_coordinator = Arc::clone(self);
        let uninstall_coordinator = Arc::clone(self);
        let install_app = app.clone();
        let uninstall_app = app.clone();
        self.apply_hotkey_settings_transaction(
            change,
            (
                |shortcut| global_shortcut.register(shortcut).map_err(|_| ()),
                |shortcut| global_shortcut.unregister(shortcut).map_err(|_| ()),
            ),
            (
                move |modifier| {
                    install_coordinator.install_production_hook_on_main(&install_app, modifier)
                },
                move || uninstall_coordinator.uninstall_production_hook_on_main(&uninstall_app),
            ),
            (
                || autostart.is_enabled().map_err(|_| ()),
                |enabled| {
                    if enabled {
                        autostart.enable()
                    } else {
                        autostart.disable()
                    }
                    .map_err(|_| ())
                },
            ),
            persist,
        )
    }

    fn install_production_hook_on_main(
        self: &Arc<Self>,
        app: &AppHandle,
        modifier: DoubleTapModifier,
    ) -> Result<(), ()> {
        let dispatcher = app.clone();
        let app = app.clone();
        let coordinator = Arc::clone(self);
        dispatch_and_wait(
            move |operation| dispatcher.run_on_main_thread(operation).map_err(|_| ()),
            move || coordinator.install_production_hook(&app, &coordinator, modifier),
        )
    }

    fn uninstall_production_hook_on_main(self: &Arc<Self>, app: &AppHandle) -> Result<(), ()> {
        let dispatcher = app.clone();
        let coordinator = Arc::clone(self);
        dispatch_and_wait(
            move |operation| dispatcher.run_on_main_thread(operation).map_err(|_| ()),
            move || coordinator.uninstall_production_hook(),
        )
    }

    fn install_production_hook(
        &self,
        app: &AppHandle,
        coordinator: &Arc<LifecycleCoordinator>,
        modifier: DoubleTapModifier,
    ) -> Result<(), ()> {
        self.uninstall_production_hook()?;
        let app_for_callback = app.clone();
        let coordinator = Arc::clone(coordinator);
        let on_match = Arc::new(move || {
            let _ = coordinator.request_show(&app_for_callback, ShowTarget::Launcher);
        });
        let hook = HotkeyHook::install(app, modifier, on_match)?;
        *self.hotkey_hook.lock().map_err(|_| ())? = Some(hook);
        Ok(())
    }

    fn uninstall_production_hook(&self) -> Result<(), ()> {
        uninstall_slot_with(&self.hotkey_hook, HotkeyHook::uninstall)
    }

    pub(crate) fn uninstall_hook_for_exit(&self) {
        let _ = self.uninstall_production_hook();
    }

    fn apply_hotkey_settings_transaction<R, U, IH, UH, A, C, P>(
        &self,
        change: HotkeyBindingChange,
        shortcut_ops: (R, U),
        hook_ops: (IH, UH),
        autostart_ops: (A, C),
        persist: P,
    ) -> Result<(), ()>
    where
        R: FnMut(Shortcut) -> Result<(), ()>,
        U: FnMut(Shortcut) -> Result<(), ()>,
        IH: FnMut(DoubleTapModifier) -> Result<(), ()>,
        UH: FnMut() -> Result<(), ()>,
        A: FnOnce() -> Result<bool, ()>,
        C: FnMut(bool) -> Result<(), ()>,
        P: FnOnce() -> Result<(), ()>,
    {
        let mut runtime = self
            .runtime_settings
            .lock()
            .expect("runtime settings lock poisoned");
        runtime.apply_hotkey_binding(change, shortcut_ops, hook_ops, autostart_ops, persist)
    }

    pub(crate) fn reconcile_runtime_settings(
        self: &Arc<Self>,
        app: &AppHandle,
        settings: &Settings,
    ) -> Result<(), ()> {
        let global_shortcut = app.global_shortcut();
        let autostart = app.autolaunch();
        let coordinator = Arc::clone(self);
        self.reconcile_runtime_settings_with(
            &settings.hotkey,
            settings.autostart,
            (
                HotkeyKind::parse,
                |shortcut| global_shortcut.register(shortcut).map_err(|_| ()),
                move |modifier| self.install_production_hook(app, &coordinator, modifier),
            ),
            (
                || autostart.is_enabled().map_err(|_| ()),
                |enabled| {
                    if enabled {
                        autostart.enable()
                    } else {
                        autostart.disable()
                    }
                    .map_err(|_| ())
                },
            ),
        )
    }

    pub(crate) fn reconcile_runtime_settings_with<P, R, IH, A, C>(
        &self,
        hotkey: &str,
        expected_autostart: bool,
        hotkey_ops: (P, R, IH),
        autostart_ops: (A, C),
    ) -> Result<(), ()>
    where
        P: FnOnce(&str) -> Result<HotkeyKind, ()>,
        R: FnOnce(Shortcut) -> Result<(), ()>,
        IH: FnOnce(DoubleTapModifier) -> Result<(), ()>,
        A: FnOnce() -> Result<bool, ()>,
        C: FnOnce(bool) -> Result<(), ()>,
    {
        let (parse, register_shortcut, install_hook) = hotkey_ops;
        let (read_autostart, change_autostart) = autostart_ops;
        let result = (|| {
            let kind = parse(hotkey)?;
            match kind {
                HotkeyKind::DoubleTap(modifier) => {
                    install_hook(modifier)?;
                    self.runtime_settings
                        .lock()
                        .expect("runtime settings lock poisoned")
                        .installed_hook = Some(modifier);
                }
                HotkeyKind::Chord(shortcut) => {
                    register_shortcut(shortcut)?;
                    self.runtime_settings
                        .lock()
                        .expect("runtime settings lock poisoned")
                        .registered
                        .push(shortcut);
                }
            }
            let actual_autostart = read_autostart()?;
            if actual_autostart != expected_autostart {
                change_autostart(expected_autostart)?;
            }
            Ok(())
        })();
        if result.is_err() {
            self.set_notice_once(LifecycleNotice::SettingsFailed);
        }
        result
    }

    pub(crate) fn request_show(
        self: &Arc<Self>,
        app: &AppHandle,
        target: ShowTarget,
    ) -> Result<(), LifecycleError> {
        let dispatcher = app.clone();
        let app_for_show = app.clone();
        self.request_show_with(target, move |coordinator, target| {
            dispatcher
                .run_on_main_thread(move || {
                    coordinator.handle_request_main(&app_for_show, target);
                })
                .map_err(|_| ())
        })
    }

    pub(crate) fn mark_setup_ready(
        self: &Arc<Self>,
        app: &AppHandle,
    ) -> Result<(), LifecycleError> {
        self.mark_setup_ready_with(|target| self.show_main(app, target))
    }

    pub(crate) fn mark_frontend_ready(
        self: &Arc<Self>,
        app: &AppHandle,
    ) -> Result<(), LifecycleError> {
        self.mark_frontend_ready_with(|target| self.show_main(app, target))
    }

    fn request_show_with<D>(
        self: &Arc<Self>,
        target: ShowTarget,
        dispatch: D,
    ) -> Result<(), LifecycleError>
    where
        D: FnOnce(Arc<Self>, ShowTarget) -> Result<(), ()>,
    {
        dispatch(Arc::clone(self), target).map_err(|_| LifecycleError::MainThreadDispatchFailed)
    }

    fn handle_request_main(self: &Arc<Self>, app: &AppHandle, target: ShowTarget) {
        let target = self
            .readiness
            .lock()
            .expect("readiness lock poisoned")
            .request(target);
        if let Some(target) = target {
            let _ = self.show_main(app, target);
        }
    }

    fn mark_setup_ready_with<S>(&self, show: S) -> Result<(), LifecycleError>
    where
        S: FnOnce(ShowTarget) -> Result<ShowOutcome, LifecycleError>,
    {
        let target = self
            .readiness
            .lock()
            .expect("readiness lock poisoned")
            .mark_setup_ready();
        if let Some(target) = target {
            show(target)?;
        }
        Ok(())
    }

    fn mark_frontend_ready_with<S>(&self, show: S) -> Result<(), LifecycleError>
    where
        S: FnOnce(ShowTarget) -> Result<ShowOutcome, LifecycleError>,
    {
        let target = self
            .readiness
            .lock()
            .expect("readiness lock poisoned")
            .mark_frontend_ready();
        if let Some(target) = target {
            show(target)?;
        }
        Ok(())
    }

    pub(crate) fn begin_modal_export<F>(
        self: &Arc<Self>,
        query_focus: F,
    ) -> Result<ModalGuard, FocusDecision>
    where
        F: FnOnce() -> Result<bool, ()>,
    {
        let query_required = self
            .modal
            .lock()
            .expect("modal lock poisoned")
            .claim_export()?;
        if query_required {
            let rollback = ModalRecoveryClaim::new(self);
            let focused = query_focus();
            self.modal
                .lock()
                .expect("modal lock poisoned")
                .resolve_export_focus(focused)?;
            rollback.commit();
        }
        Ok(ModalGuard {
            coordinator: Arc::clone(self),
        })
    }

    pub(crate) fn handle_focus_event_with<Q, H>(
        &self,
        focused: bool,
        query_focus: Q,
        mut clear_and_hide: H,
    ) -> Result<(), LifecycleError>
    where
        Q: FnOnce() -> Result<bool, ()>,
        H: FnMut() -> Result<(), ()>,
    {
        let decision = self
            .modal
            .lock()
            .expect("modal lock poisoned")
            .begin_focus(focused);
        let decision = decision.unwrap_or_else(|| match query_focus() {
            Ok(true) => FocusDecision::Suppress,
            Ok(false) => FocusDecision::ClearAndHide,
            Err(()) => FocusDecision::ReportWindowFailureAndHide,
        });
        match decision {
            FocusDecision::Suppress => Ok(()),
            FocusDecision::ClearAndHide => {
                clear_and_hide().map_err(|_| LifecycleError::WindowFailed)
            }
            FocusDecision::ReportWindowFailureAndHide => {
                let _ = clear_and_hide();
                Err(LifecycleError::WindowFailed)
            }
        }
    }

    pub(crate) fn on_successful_show(&self) {
        self.modal
            .lock()
            .expect("modal lock poisoned")
            .on_successful_show();
    }

    fn show_main(
        self: &Arc<Self>,
        app: &AppHandle,
        target: ShowTarget,
    ) -> Result<ShowOutcome, LifecycleError> {
        let Some((window, registry)) = self.show_main_with_resolver(
            || self.observe_exit(),
            || {
                let window = app
                    .get_webview_window("main")
                    .ok_or(LifecycleError::WindowFailed)?;
                let registry = app.state::<ResultRegistry>();
                Ok((window, registry))
            },
        )?
        else {
            return Ok(ShowOutcome::Ignored);
        };

        let mut center = || window.center().map_err(|_| ());
        let mut always_on_top = || window.set_always_on_top(true).map_err(|_| ());
        let mut show = || window.show().map_err(|_| ());
        let mut focus = || window.set_focus().map_err(|_| ());
        let mut registry_on_show = |invocation_id| registry.on_show(invocation_id);
        let mut emit =
            |payload: &LauncherShown| window.emit("launcher://shown", payload).map_err(|_| ());
        let mut clear_window = || {
            let _ = clear_and_hide(&registry, &window);
        };
        let app_for_record = app.clone();
        let coordinator_for_record = Arc::clone(self);
        let mut record_launcher = move |reservation| {
            let app = app_for_record.clone();
            let coordinator = Arc::clone(&coordinator_for_record);
            drop(tauri::async_runtime::spawn_blocking(move || {
                let _reservation = reservation;
                let result = app
                    .state::<ValidationStore>()
                    .record(ValidationEvent::LauncherInvoked)
                    .map_err(|_| ());
                coordinator.finish_launcher_record(result);
            }));
        };
        let mut operations = ShowMainClosures {
            center: &mut center,
            always_on_top: &mut always_on_top,
            show: &mut show,
            focus: &mut focus,
            registry_on_show: &mut registry_on_show,
            emit: &mut emit,
            clear_and_hide: &mut clear_window,
            record_launcher: &mut record_launcher,
        };
        self.show_main_core(target, &mut operations)
    }

    fn show_main_with_resolver<T, O, R>(
        &self,
        observe_exit: O,
        resolve: R,
    ) -> Result<Option<T>, LifecycleError>
    where
        O: FnOnce() -> ExitState,
        R: FnOnce() -> Result<T, LifecycleError>,
    {
        if observe_exit() != ExitState::Running {
            return Ok(None);
        }
        resolve().map(Some)
    }

    fn show_main_core(
        self: &Arc<Self>,
        target: ShowTarget,
        operations: &mut ShowMainClosures<'_>,
    ) -> Result<ShowOutcome, LifecycleError> {
        let previous = self
            .next_invocation
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| LifecycleError::InvocationExhausted)?;
        let invocation_id = format!("invocation-{}", previous + 1);

        let _ = (operations.center)();
        for operation in [
            &mut operations.always_on_top,
            &mut operations.show,
            &mut operations.focus,
        ] {
            if operation().is_err() {
                (operations.clear_and_hide)();
                return Err(LifecycleError::WindowFailed);
            }
        }

        (operations.registry_on_show)(invocation_id.clone());
        let notice = *self
            .pending_notice
            .lock()
            .expect("pending notice lock poisoned");
        let payload = LauncherShown {
            invocation_id,
            target,
            notice,
        };
        if (operations.emit)(&payload).is_err() {
            (operations.clear_and_hide)();
            return Err(LifecycleError::WindowFailed);
        }
        self.consume_notice(notice);
        self.on_successful_show();

        if target == ShowTarget::Launcher {
            match self.reserve_critical() {
                Ok(reservation) => (operations.record_launcher)(reservation),
                Err(_) => self.set_notice_once(LifecycleNotice::ValidationFailed),
            }
        }
        Ok(ShowOutcome::Shown)
    }

    fn finish_launcher_record(&self, result: Result<(), ()>) {
        if result.is_err() {
            self.set_notice_once(LifecycleNotice::ValidationFailed);
        }
    }

    fn set_notice_once(&self, notice: LifecycleNotice) {
        let mut pending = self
            .pending_notice
            .lock()
            .expect("pending notice lock poisoned");
        if pending.is_none() {
            *pending = Some(notice);
        }
    }

    fn consume_notice(&self, notice: Option<LifecycleNotice>) {
        let Some(notice) = notice else {
            return;
        };
        let mut pending = self
            .pending_notice
            .lock()
            .expect("pending notice lock poisoned");
        if *pending == Some(notice) {
            *pending = None;
        }
    }

    pub(crate) fn reserve_critical(
        self: &Arc<Self>,
    ) -> Result<CriticalReservation, ReservationError> {
        let mut gate = self.exit_gate.lock().expect("exit gate lock poisoned");
        if gate.state != ExitState::Running {
            return Err(ReservationError::NotRunning);
        }
        gate.in_flight_critical = gate
            .in_flight_critical
            .checked_add(1)
            .ok_or(ReservationError::Overflow)?;

        Ok(CriticalReservation {
            coordinator: Arc::clone(self),
            released: false,
        })
    }

    fn begin_tray_clean_start(&self, deadline: Instant) -> TrayCleanStart {
        let mut gate = self.exit_gate.lock().expect("exit gate lock poisoned");
        if gate.state != ExitState::Running || !matches!(gate.clean_attempt, CleanAttempt::Idle) {
            return TrayCleanStart {
                decision: CleanDecision::ObserveOnly,
                attempt_epoch: self.lifecycle_attempt_epoch.load(Ordering::Relaxed),
                attempt_overflowed: false,
                deadline,
            };
        }

        let previous = self.lifecycle_attempt_epoch.load(Ordering::Relaxed);
        let (attempt, attempt_overflowed) = match previous.checked_add(1) {
            Some(attempt) => (attempt, false),
            None => (previous, true),
        };
        self.lifecycle_attempt_epoch
            .store(attempt, Ordering::Release);
        gate.state = ExitState::Cleaning;
        self.store_file_index_phase(FileIndexPhase::Cleaning);
        TrayCleanStart {
            decision: Self::start_waiting(&mut gate, CleanOwner::Tray, deadline),
            attempt_epoch: attempt,
            attempt_overflowed,
            deadline,
        }
    }

    #[cfg(test)]
    fn begin_tray_clean(&self, deadline: Instant) -> CleanDecision {
        self.begin_tray_clean_start(deadline).decision
    }

    fn begin_system_end_nonblocking(&self, now: Instant) -> CleanDecision {
        let mut gate = self.exit_gate.lock().expect("exit gate lock poisoned");
        gate.state = ExitState::SystemEnding;
        self.store_file_index_phase(FileIndexPhase::Terminal);

        match gate.clean_attempt {
            CleanAttempt::Idle | CleanAttempt::Waiting { .. } if gate.in_flight_critical == 0 => {
                gate.clean_attempt = CleanAttempt::Calling {
                    owner: CleanOwner::System,
                    deadline: now,
                };
                CleanDecision::CallMarker
            }
            CleanAttempt::Idle | CleanAttempt::Waiting { .. } => {
                gate.clean_attempt = CleanAttempt::Finished(CleanResult::TimedOut);
                drop(gate);
                self.critical_changed.notify_all();
                CleanDecision::ObserveOnly
            }
            CleanAttempt::Calling { .. } | CleanAttempt::Finished(_) => CleanDecision::ObserveOnly,
        }
    }

    fn advance_clean(&self, now: Instant) -> CleanDecision {
        let mut gate = self.exit_gate.lock().expect("exit gate lock poisoned");
        match gate.clean_attempt {
            CleanAttempt::Waiting { owner, deadline } if now >= deadline => {
                let decision = if owner == CleanOwner::Tray && gate.state == ExitState::Cleaning {
                    gate.state = ExitState::Running;
                    gate.clean_attempt = CleanAttempt::Idle;
                    self.store_file_index_phase(FileIndexPhase::Running);
                    CleanDecision::ReturnRunning
                } else {
                    gate.clean_attempt = CleanAttempt::Finished(CleanResult::TimedOut);
                    CleanDecision::ObserveOnly
                };
                drop(gate);
                self.critical_changed.notify_all();
                decision
            }
            CleanAttempt::Waiting { owner, deadline } if gate.in_flight_critical == 0 => {
                gate.clean_attempt = CleanAttempt::Calling { owner, deadline };
                CleanDecision::CallMarker
            }
            CleanAttempt::Waiting { deadline, .. } => CleanDecision::Wait { deadline },
            CleanAttempt::Calling { deadline, .. } if now < deadline => {
                CleanDecision::Wait { deadline }
            }
            CleanAttempt::Calling { .. } | CleanAttempt::Idle | CleanAttempt::Finished(_) => {
                CleanDecision::ObserveOnly
            }
        }
    }

    fn complete_clean(&self, result: CleanResult) -> CleanDecision {
        let mut gate = self.exit_gate.lock().expect("exit gate lock poisoned");
        let CleanAttempt::Calling { owner, .. } = gate.clean_attempt else {
            return CleanDecision::ObserveOnly;
        };

        let decision = if gate.state == ExitState::SystemEnding {
            gate.clean_attempt = CleanAttempt::Finished(result);
            CleanDecision::ObserveOnly
        } else if gate.state == ExitState::Cleaning && owner == CleanOwner::Tray {
            match result {
                CleanResult::Succeeded => {
                    gate.state = ExitState::Clean;
                    gate.clean_attempt = CleanAttempt::Finished(result);
                    self.store_file_index_phase(FileIndexPhase::Terminal);
                    CleanDecision::Exit
                }
                CleanResult::Failed | CleanResult::TimedOut => {
                    gate.state = ExitState::Running;
                    gate.clean_attempt = CleanAttempt::Idle;
                    self.store_file_index_phase(FileIndexPhase::Running);
                    CleanDecision::ReturnRunning
                }
            }
        } else {
            CleanDecision::ObserveOnly
        };

        drop(gate);
        self.critical_changed.notify_all();
        decision
    }

    fn run_clean_attempt_with<W, M>(
        &self,
        mut decision: CleanDecision,
        mut wait: W,
        marker: M,
    ) -> CleanDecision
    where
        W: FnMut(Instant) -> Instant,
        M: FnOnce() -> CleanResult,
    {
        let mut marker = Some(marker);
        loop {
            decision = match decision {
                CleanDecision::Wait { deadline } => self.advance_clean(wait(deadline)),
                CleanDecision::CallMarker => {
                    return self.complete_clean(marker.take().expect("marker called once")());
                }
                decision => return decision,
            };
        }
    }

    fn run_tray_quit_with<W, M, E, S>(
        &self,
        decision: CleanDecision,
        wait: W,
        marker: M,
        exit: E,
        show: S,
    ) -> CleanDecision
    where
        W: FnMut(Instant) -> Instant,
        M: FnOnce() -> CleanResult,
        E: FnOnce(),
        S: FnOnce(ShowTarget),
    {
        let decision = self.run_clean_attempt_with(decision, wait, marker);
        match decision {
            CleanDecision::Exit => exit(),
            CleanDecision::ReturnRunning => {
                self.set_notice_once(LifecycleNotice::ValidationFailed);
                show(ShowTarget::Settings);
            }
            _ => {}
        }
        decision
    }

    fn run_system_end_nonblocking_with<M, T>(
        &self,
        now: Instant,
        marker: M,
        terminal: T,
    ) -> CleanDecision
    where
        M: FnOnce() -> CleanResult,
        T: FnOnce(),
    {
        let decision = self.begin_system_end_nonblocking(now);
        terminal();
        match decision {
            CleanDecision::CallMarker => self.complete_clean(marker()),
            decision => decision,
        }
    }

    fn wait_for_clean_change(&self, deadline: Instant) -> Instant {
        let gate = self.exit_gate.lock().expect("exit gate lock poisoned");
        let now = Instant::now();
        if now >= deadline {
            return now;
        }
        let _ = self
            .critical_changed
            .wait_timeout_while(
                gate,
                deadline.saturating_duration_since(now),
                |gate| match gate.clean_attempt {
                    CleanAttempt::Waiting { .. } => gate.in_flight_critical != 0,
                    CleanAttempt::Calling { .. } => true,
                    CleanAttempt::Idle | CleanAttempt::Finished(_) => false,
                },
            )
            .expect("exit gate lock poisoned");
        Instant::now()
    }

    pub(crate) fn request_tray_quit(self: &Arc<Self>, app: &AppHandle) {
        let start = self.begin_tray_clean_start(Instant::now() + Duration::from_secs(5));
        if start.decision == CleanDecision::ObserveOnly {
            return;
        }
        let file_index = Arc::clone(app.state::<Arc<FileIndex>>().inner());
        if start.attempt_overflowed {
            file_index.fail_closed_exhaustion();
        } else {
            let _ = file_index.start_cleaning_until(start.attempt_epoch, start.deadline);
        }

        let coordinator = Arc::clone(self);
        let app = app.clone();
        drop(tauri::async_runtime::spawn_blocking(move || {
            let marker_app = app.clone();
            let marker_index = Arc::clone(&file_index);
            let exit_dispatcher = app.clone();
            let exit_app = app.clone();
            let exit_coordinator = Arc::clone(&coordinator);
            let exit_index = Arc::clone(&file_index);
            let show_app = app.clone();
            let show_coordinator = Arc::clone(&coordinator);
            let show_index = Arc::clone(&file_index);
            coordinator.run_tray_quit_with(
                start.decision,
                |deadline| coordinator.wait_for_clean_change(deadline),
                || {
                    if marker_index.mark_clean_close(start.attempt_epoch)
                        && marker_app
                            .state::<ValidationStore>()
                            .mark_clean_exit()
                            .is_ok()
                    {
                        CleanResult::Succeeded
                    } else {
                        CleanResult::Failed
                    }
                },
                move || {
                    exit_index.enter_terminal();
                    exit_coordinator.uninstall_hook_for_exit();
                    let app = exit_app.clone();
                    let _ = exit_dispatcher.run_on_main_thread(move || app.exit(0));
                },
                move |target| {
                    let _ = show_index.return_running(start.attempt_epoch);
                    let _ = show_coordinator.request_show(&show_app, target);
                },
            );
        }));
    }

    fn run_system_end(&self, app: &AppHandle) {
        let marker_app = app.clone();
        let terminal_index = Arc::clone(app.state::<Arc<FileIndex>>().inner());
        let _ = self.run_system_end_nonblocking_with(
            Instant::now(),
            || {
                if marker_app
                    .state::<ValidationStore>()
                    .mark_clean_exit()
                    .is_ok()
                {
                    CleanResult::Succeeded
                } else {
                    CleanResult::Failed
                }
            },
            move || terminal_index.enter_terminal(),
        );
    }

    pub(crate) fn should_prevent_exit(&self) -> bool {
        matches!(
            self.observe_exit(),
            ExitState::Running | ExitState::Cleaning
        )
    }

    pub(crate) fn should_prevent_close(&self) -> bool {
        self.should_prevent_exit()
    }

    pub(crate) fn observe_run_exit(&self) {
        let _ = self.observe_exit();
    }

    fn observe_exit(&self) -> ExitState {
        self.exit_gate
            .lock()
            .expect("exit gate lock poisoned")
            .state
    }

    fn start_waiting(gate: &mut ExitGate, owner: CleanOwner, deadline: Instant) -> CleanDecision {
        if gate.in_flight_critical == 0 {
            gate.clean_attempt = CleanAttempt::Calling { owner, deadline };
            CleanDecision::CallMarker
        } else {
            gate.clean_attempt = CleanAttempt::Waiting { owner, deadline };
            CleanDecision::Wait { deadline }
        }
    }
}

fn handle_session_message_with<D, C, R>(
    message: u32,
    wparam: usize,
    mut default: D,
    clean: C,
    destroy: R,
) -> isize
where
    D: FnMut() -> isize,
    C: FnOnce(),
    R: FnOnce(),
{
    match message {
        WM_QUERYENDSESSION => default(),
        WM_ENDSESSION => {
            if wparam != 0 {
                clean();
            }
            let _ = default();
            0
        }
        WM_NCDESTROY => {
            destroy();
            default()
        }
        _ => default(),
    }
}

fn install_subclass_context_with<T, I>(context: T, install: I) -> Result<usize, ()>
where
    I: FnOnce(usize) -> bool,
{
    let raw = Box::into_raw(Box::new(context)) as usize;
    if install(raw) {
        Ok(raw)
    } else {
        unsafe { reclaim_subclass_context::<T>(raw) };
        Err(())
    }
}

unsafe fn reclaim_subclass_context<T>(raw: usize) {
    drop(unsafe { Box::from_raw(raw as *mut T) });
}

unsafe fn remove_subclass_context_with<T, R>(raw: usize, remove: R) -> bool
where
    R: FnOnce() -> bool,
{
    let removed = remove();
    if removed {
        unsafe { reclaim_subclass_context::<T>(raw) };
    }
    removed
}

unsafe extern "system" fn session_subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    context: usize,
) -> LRESULT {
    let app = unsafe { (&*(context as *const AppHandle)).clone() };
    LRESULT(handle_session_message_with(
        message,
        wparam.0,
        || unsafe { DefSubclassProc(hwnd, message, wparam, lparam).0 },
        || {
            app.state::<Arc<LifecycleCoordinator>>()
                .run_system_end(&app);
        },
        || {
            app.state::<Arc<FileIndex>>()
                .clear_main_window_hwnd(hwnd.0 as isize);
            let _ = unsafe {
                remove_subclass_context_with::<AppHandle, _>(context, || {
                    RemoveWindowSubclass(hwnd, Some(session_subclass_proc), SESSION_SUBCLASS_ID)
                        .as_bool()
                })
            };
        },
    ))
}

pub(crate) fn install_session_end_hook(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
) -> Result<(), LifecycleError> {
    let hwnd = window
        .hwnd()
        .map_err(|_| LifecycleError::SessionHookFailed)?;
    install_subclass_context_with(app.clone(), |context| unsafe {
        SetWindowSubclass(
            hwnd,
            Some(session_subclass_proc),
            SESSION_SUBCLASS_ID,
            context,
        )
        .as_bool()
    })
    .map(|_| ())
    .map_err(|_| LifecycleError::SessionHookFailed)
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        panic::{catch_unwind, AssertUnwindSafe},
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Barrier,
        },
        thread,
        time::{Duration, Instant},
    };

    use super::*;
    use crate::{
        commands::save_settings_worker_with,
        hotkey::{DoubleTapModifier, HotkeyKind, DOUBLE_ALT, DOUBLE_CTRL},
        result_registry::{QueryDomain, ResultRegistry},
    };
    use tauri_plugin_global_shortcut::Shortcut;

    const _: fn(&Arc<LifecycleCoordinator>, &AppHandle, ShowTarget) -> Result<(), LifecycleError> =
        LifecycleCoordinator::request_show;
    const _: fn(&Arc<LifecycleCoordinator>, &AppHandle) -> Result<(), LifecycleError> =
        LifecycleCoordinator::mark_setup_ready;
    const _: fn(&Arc<LifecycleCoordinator>, &AppHandle) = LifecycleCoordinator::request_tray_quit;
    const _: fn(&Arc<LifecycleCoordinator>, &AppHandle, &Settings) -> Result<(), ()> =
        LifecycleCoordinator::reconcile_runtime_settings;

    #[derive(Clone, Copy)]
    enum ReadyOrder {
        SetupFirst,
        FrontendFirst,
    }

    fn apply_ready_order(state: &mut Readiness, order: ReadyOrder) -> Vec<ShowTarget> {
        let results = match order {
            ReadyOrder::SetupFirst => [state.mark_setup_ready(), state.mark_frontend_ready()],
            ReadyOrder::FrontendFirst => [state.mark_frontend_ready(), state.mark_setup_ready()],
        };
        results.into_iter().flatten().collect()
    }

    #[test]
    fn dispatch_and_wait_returns_operation_result_from_dispatch_thread() {
        let caller = thread::current().id();
        let observed = Arc::new(Mutex::new(None));
        let observed_for_operation = Arc::clone(&observed);

        assert_eq!(
            dispatch_and_wait(
                |operation| {
                    thread::spawn(operation).join().map_err(|_| ())?;
                    Ok(())
                },
                move || {
                    *observed_for_operation.lock().unwrap() = Some(thread::current().id());
                    Ok(())
                },
            ),
            Ok(())
        );
        assert_ne!(*observed.lock().unwrap(), Some(caller));
    }

    #[test]
    fn dispatch_and_wait_propagates_operation_failure() {
        assert_eq!(
            dispatch_and_wait(
                |operation| {
                    operation();
                    Ok(())
                },
                || Err(()),
            ),
            Err(())
        );
    }

    #[test]
    fn dispatch_and_wait_stops_on_dispatch_rejection() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_operation = Arc::clone(&calls);

        assert_eq!(
            dispatch_and_wait(
                |_| Err(()),
                move || {
                    calls_for_operation.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                },
            ),
            Err(())
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn dispatch_and_wait_maps_dropped_operation_to_error() {
        assert_eq!(
            dispatch_and_wait(
                |operation| {
                    drop(operation);
                    Ok(())
                },
                || Ok(()),
            ),
            Err(())
        );
    }

    #[test]
    fn uninstall_slot_reinserts_failed_handle_and_retries() {
        let slot = Mutex::new(Some("handle"));

        assert_eq!(uninstall_slot_with(&slot, Err), Err(()));
        assert_eq!(*slot.lock().unwrap(), Some("handle"));

        assert_eq!(uninstall_slot_with(&slot, |_| Ok(())), Ok(()));
        assert_eq!(*slot.lock().unwrap(), None);
    }

    #[test]
    fn system_end_returns_while_save_waits_for_main_thread() {
        let coordinator = coordinator_for_test();
        let worker_coordinator = Arc::clone(&coordinator);
        let (operation_sender, operation_receiver) = std::sync::mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let _reservation = worker_coordinator.reserve_critical().unwrap();
            let _runtime = worker_coordinator.runtime_settings.lock().unwrap();
            dispatch_and_wait(
                move |operation| operation_sender.send(operation).map_err(|_| ()),
                || Ok(()),
            )
        });
        let operation = operation_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("save worker must reach the main-thread wait");

        let system_coordinator = Arc::clone(&coordinator);
        let marker_calls = Arc::new(AtomicUsize::new(0));
        let system_marker_calls = Arc::clone(&marker_calls);
        let (decision_sender, decision_receiver) = std::sync::mpsc::sync_channel(1);
        let system = thread::spawn(move || {
            let decision = system_coordinator.run_system_end_nonblocking_with(
                Instant::now(),
                || {
                    system_marker_calls.fetch_add(1, Ordering::Relaxed);
                    CleanResult::Succeeded
                },
                || {},
            );
            decision_sender.send(decision).unwrap();
        });

        let decision_before_operation = decision_receiver.recv_timeout(Duration::from_secs(1));
        operation();
        assert_eq!(worker.join().unwrap(), Ok(()));
        let decision = decision_before_operation
            .or_else(|_| decision_receiver.recv_timeout(Duration::from_secs(1)));
        system.join().unwrap();

        assert_eq!(
            decision.expect("system end must return before the main-thread operation runs"),
            CleanDecision::ObserveOnly
        );
        assert_eq!(marker_calls.load(Ordering::Relaxed), 0);
        assert_eq!(
            exit_snapshot(&coordinator),
            (
                ExitState::SystemEnding,
                0,
                CleanAttempt::Finished(CleanResult::TimedOut)
            )
        );
    }

    #[test]
    fn exit_uninstall_does_not_wait_for_runtime_settings_lock() {
        let coordinator = coordinator_for_test();
        let holder_coordinator = Arc::clone(&coordinator);
        let (held_sender, held_receiver) = std::sync::mpsc::sync_channel(1);
        let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(1);
        let holder = thread::spawn(move || {
            let _runtime = holder_coordinator.runtime_settings.lock().unwrap();
            held_sender.send(()).unwrap();
            release_receiver.recv().unwrap();
        });
        held_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("runtime settings lock must be held");

        let exit_coordinator = Arc::clone(&coordinator);
        let (done_sender, done_receiver) = std::sync::mpsc::sync_channel(1);
        let uninstaller = thread::spawn(move || {
            exit_coordinator.uninstall_hook_for_exit();
            done_sender.send(()).unwrap();
        });

        let completed_while_locked = done_receiver.recv_timeout(Duration::from_secs(1));
        release_sender.send(()).unwrap();
        holder.join().unwrap();
        uninstaller.join().unwrap();
        assert!(
            completed_while_locked.is_ok(),
            "exit uninstall must not acquire runtime_settings"
        );
    }

    #[test]
    fn file_index_handlers_run_after_exit_lock_release() {
        let source = include_str!("lifecycle.rs").replace("\r\n", "\n");
        let production = source.split("#[cfg(test)]\nmod tests").next().unwrap();

        let tray = production
            .split("pub(crate) fn request_tray_quit")
            .nth(1)
            .and_then(|tail| tail.split("fn run_system_end(").next())
            .expect("tray quit source markers are missing");
        let start = tray
            .find("let start = self.begin_tray_clean_start")
            .unwrap();
        let file_index = tray.find("let file_index = Arc::clone").unwrap();
        let cleaning = tray.find("file_index.start_cleaning_until(").unwrap();
        let spawn = tray.find("spawn_blocking").unwrap();
        assert!(start < file_index && file_index < cleaning && cleaning < spawn);
        assert!(tray.contains("show_index.return_running(start.attempt_epoch);"));
        assert!(tray.contains("exit_index.enter_terminal();"));

        let system_end = production
            .split("fn run_system_end(")
            .nth(1)
            .and_then(|tail| tail.split("pub(crate) fn should_prevent_exit").next())
            .expect("system end source markers are missing");
        assert!(system_end.contains("run_system_end_nonblocking_with"));
        assert!(system_end.contains("terminal_index.enter_terminal()"));
        assert!(!system_end.contains("wait_for_clean_change"));
    }

    #[test]
    fn production_wiring_uses_main_wrappers_only_for_dynamic_save() {
        let source = include_str!("lifecycle.rs").replace("\r\n", "\n");
        let production = source.split("#[cfg(test)]\nmod tests").next().unwrap();
        let save = production
            .split("pub(crate) fn save_settings_transaction")
            .nth(1)
            .and_then(|tail| tail.split("fn install_production_hook(").next())
            .expect("save transaction source markers are missing");
        assert!(save.contains("install_production_hook_on_main"));
        assert!(save.contains("uninstall_production_hook_on_main"));

        let reconcile = production
            .split("pub(crate) fn reconcile_runtime_settings(")
            .nth(1)
            .and_then(|tail| {
                tail.split("pub(crate) fn reconcile_runtime_settings_with")
                    .next()
            })
            .expect("reconcile source markers are missing");
        assert!(reconcile.contains("install_production_hook("));
        assert!(!reconcile.contains("install_production_hook_on_main"));

        let tray = production
            .split("pub(crate) fn request_tray_quit")
            .nth(1)
            .and_then(|tail| tail.split("fn run_system_end(").next())
            .expect("tray quit source markers are missing");
        let marker = tray.find("mark_clean_exit").unwrap();
        let uninstall = tray.find("uninstall_hook_for_exit").unwrap();
        assert!(marker < uninstall);
        assert_eq!(tray.matches("uninstall_hook_for_exit").count(), 1);

        let system_end = production
            .split("fn run_system_end(")
            .nth(1)
            .and_then(|tail| tail.split("pub(crate) fn should_prevent_exit").next())
            .expect("system end source markers are missing");
        assert!(system_end.contains("run_system_end_nonblocking_with"));
        assert!(!system_end.contains("wait_for_clean_change"));
        assert!(!system_end.contains("uninstall_hook_for_exit"));

        let session_callback = production
            .split("unsafe extern \"system\" fn session_subclass_proc")
            .nth(1)
            .and_then(|tail| tail.split("pub(crate) fn install_session_end_hook").next())
            .expect("session callback source markers are missing");
        assert!(session_callback.contains("run_system_end(&app)"));
        assert!(session_callback.contains("clear_main_window_hwnd(hwnd.0 as isize)"));
        assert!(!session_callback.contains("uninstall_hook_for_exit"));
        assert!(!session_callback.contains("wait_for_clean_change"));
    }

    #[test]
    fn readiness_does_not_synthesize_a_startup_show() {
        for order in [ReadyOrder::SetupFirst, ReadyOrder::FrontendFirst] {
            let mut state = Readiness::default();
            assert_eq!(
                apply_ready_order(&mut state, order),
                Vec::<ShowTarget>::new()
            );
            assert_eq!(state.pending_target, None);
        }
    }

    #[test]
    fn readiness_drains_only_the_last_early_explicit_target_once() {
        for (target, order) in [
            (ShowTarget::Launcher, ReadyOrder::SetupFirst),
            (ShowTarget::Settings, ReadyOrder::FrontendFirst),
        ] {
            let mut state = Readiness::default();
            assert_eq!(state.request(target), None);
            assert_eq!(apply_ready_order(&mut state, order), vec![target]);
            assert_eq!(state.mark_setup_ready(), None);
            assert_eq!(state.mark_frontend_ready(), None);
        }

        let mut state = Readiness::default();
        assert_eq!(state.request(ShowTarget::Launcher), None);
        assert_eq!(state.request(ShowTarget::Settings), None);
        assert_eq!(state.mark_setup_ready(), None);
        assert_eq!(state.mark_frontend_ready(), Some(ShowTarget::Settings));
        assert_eq!(state.mark_frontend_ready(), None);
    }

    #[test]
    fn readiness_returns_each_request_after_both_ready() {
        let mut state = Readiness::default();
        assert!(apply_ready_order(&mut state, ReadyOrder::SetupFirst).is_empty());

        for target in [ShowTarget::Launcher, ShowTarget::Settings] {
            assert_eq!(state.request(target), Some(target));
            assert_eq!(state.pending_target, None);
        }
    }

    #[test]
    fn readiness_marks_are_idempotent_and_state_has_only_approved_fields() {
        let mut state = Readiness {
            setup_ready: false,
            frontend_ready: false,
            pending_target: None,
        };
        assert_eq!(state.mark_setup_ready(), None);
        assert_eq!(state.mark_setup_ready(), None);
        assert_eq!(state.request(ShowTarget::Launcher), None);
        assert_eq!(state.mark_setup_ready(), None);
        assert_eq!(state.mark_frontend_ready(), Some(ShowTarget::Launcher));
        assert_eq!(state.mark_frontend_ready(), None);
    }

    #[test]
    fn modal_normal_and_open_export_paths_allow_only_one_chooser() {
        let mut state = ModalState::Normal;
        assert_eq!(state.claim_export(), Ok(false));
        assert_eq!(state, ModalState::Open);
        assert_eq!(state.claim_export(), Err(FocusDecision::Suppress));
        assert_eq!(state, ModalState::Open);
    }

    #[test]
    fn modal_guard_drop_and_focus_events_restore_normal() {
        let mut state = ModalState::Open;
        assert_eq!(
            state.on_focus(false, || panic!("Open must not query focus")),
            FocusDecision::Suppress
        );
        assert_eq!(state, ModalState::Open);

        state.finish_export();
        assert_eq!(state, ModalState::AwaitingFocusRestore);
        assert_eq!(
            state.on_focus(true, || panic!("Focused(true) must not query focus")),
            FocusDecision::Suppress
        );
        assert_eq!(state, ModalState::Normal);
    }

    #[test]
    fn modal_awaiting_false_classifies_after_leaving_awaiting() {
        for (focus_result, expected) in [
            (Ok(true), FocusDecision::Suppress),
            (Ok(false), FocusDecision::ClearAndHide),
            (Err(()), FocusDecision::ReportWindowFailureAndHide),
        ] {
            let mut state = ModalState::AwaitingFocusRestore;
            assert_eq!(state.on_focus(false, || focus_result), expected);
            assert_eq!(state, ModalState::Normal);
        }

        let mut state = ModalState::AwaitingFocusRestore;
        let query = catch_unwind(AssertUnwindSafe(|| {
            state.on_focus(false, || panic!("focus query sentinel"));
        }));
        assert!(query.is_err());
        assert_eq!(state, ModalState::Normal);
    }

    #[test]
    fn modal_retry_and_successful_show_cannot_leave_awaiting_stuck() {
        let mut focused = ModalState::AwaitingFocusRestore;
        assert_eq!(focused.claim_export(), Ok(true));
        assert_eq!(focused.resolve_export_focus(Ok(true)), Ok(()));
        assert_eq!(focused, ModalState::Open);

        for (focus_result, expected) in [
            (Ok(false), FocusDecision::ClearAndHide),
            (Err(()), FocusDecision::ReportWindowFailureAndHide),
        ] {
            let mut state = ModalState::AwaitingFocusRestore;
            assert_eq!(state.claim_export(), Ok(true));
            assert_eq!(state.resolve_export_focus(focus_result), Err(expected));
            assert_eq!(state, ModalState::Normal);
            assert_eq!(state.claim_export(), Ok(false));
            assert_eq!(state, ModalState::Open);
        }

        let mut state = ModalState::AwaitingFocusRestore;
        state.on_successful_show();
        assert_eq!(state, ModalState::Normal);
        state = ModalState::Open;
        state.on_successful_show();
        assert_eq!(state, ModalState::Open);

        let coordinator = coordinator_for_test();
        let first = coordinator.begin_modal_export(|| Ok(true)).unwrap();
        drop(first);
        let recovered = coordinator
            .begin_modal_export(|| {
                let lock = coordinator
                    .modal
                    .try_lock()
                    .expect("focus query must run without the modal lock");
                drop(lock);
                assert_eq!(
                    coordinator
                        .begin_modal_export(|| panic!("Open must not query focus"))
                        .unwrap_err(),
                    FocusDecision::Suppress
                );
                Ok(true)
            })
            .unwrap();
        drop(recovered);

        let panic_result = catch_unwind(AssertUnwindSafe(|| {
            let _ = coordinator.begin_modal_export(|| panic!("live focus query sentinel"));
        }));
        assert!(panic_result.is_err());
        let after_panic = coordinator
            .begin_modal_export(|| panic!("rollback must restore Normal"))
            .unwrap();
        drop(after_panic);
    }

    #[derive(Clone, Copy, Eq, PartialEq)]
    enum ShowFailure {
        None,
        Center,
        AlwaysOnTop,
        Show,
        Focus,
        Emit,
        Record,
        PanicRecord,
    }

    #[derive(Default)]
    struct ShowProbe {
        trace: RefCell<Vec<String>>,
        payloads: RefCell<Vec<LauncherShown>>,
        clears: Cell<usize>,
        records: Cell<usize>,
    }

    fn run_show_case(
        coordinator: &Arc<LifecycleCoordinator>,
        target: ShowTarget,
        index: u64,
        failure: ShowFailure,
        probe: &ShowProbe,
    ) -> Result<ShowOutcome, LifecycleError> {
        let mut center = || {
            probe.trace.borrow_mut().push(format!("invocation-{index}"));
            probe.trace.borrow_mut().push(format!("center-{index}"));
            (failure != ShowFailure::Center).then_some(()).ok_or(())
        };
        let mut always_on_top = || {
            probe
                .trace
                .borrow_mut()
                .push(format!("always-on-top-{index}"));
            (failure != ShowFailure::AlwaysOnTop)
                .then_some(())
                .ok_or(())
        };
        let mut show = || {
            probe.trace.borrow_mut().push(format!("show-{index}"));
            (failure != ShowFailure::Show).then_some(()).ok_or(())
        };
        let mut focus = || {
            probe.trace.borrow_mut().push(format!("focus-{index}"));
            (failure != ShowFailure::Focus).then_some(()).ok_or(())
        };
        let mut registry_on_show = |invocation_id: String| {
            assert_eq!(invocation_id, format!("invocation-{index}"));
            probe
                .trace
                .borrow_mut()
                .push(format!("registry-on-show-{index}"));
        };
        let mut emit = |payload: &LauncherShown| {
            probe.trace.borrow_mut().push(format!("emit-{index}"));
            probe.payloads.borrow_mut().push(payload.clone());
            (failure != ShowFailure::Emit).then_some(()).ok_or(())
        };
        let mut clear_and_hide = || {
            probe.clears.set(probe.clears.get() + 1);
            probe.trace.borrow_mut().push(format!("clear-{index}"));
        };
        let coordinator_for_record = Arc::clone(coordinator);
        let mut record_launcher = move |_reservation: CriticalReservation| {
            probe.records.set(probe.records.get() + 1);
            probe
                .trace
                .borrow_mut()
                .push(format!("reserve-launcher-record-{index}"));
            match failure {
                ShowFailure::Record => coordinator_for_record.finish_launcher_record(Err(())),
                ShowFailure::PanicRecord => panic!("launcher record panic sentinel"),
                _ => coordinator_for_record.finish_launcher_record(Ok(())),
            }
        };
        let mut operations = ShowMainClosures {
            center: &mut center,
            always_on_top: &mut always_on_top,
            show: &mut show,
            focus: &mut focus,
            registry_on_show: &mut registry_on_show,
            emit: &mut emit,
            clear_and_hide: &mut clear_and_hide,
            record_launcher: &mut record_launcher,
        };
        coordinator.show_main_core(target, &mut operations)
    }

    #[test]
    fn show_serializes_two_ready_requests_in_registry_event_order() {
        let coordinator = coordinator_for_test();
        coordinator
            .mark_setup_ready_with(|_| panic!("setup ready must not synthesize show"))
            .unwrap();
        coordinator
            .mark_frontend_ready_with(|_| panic!("frontend ready must not synthesize show"))
            .unwrap();
        let probe = ShowProbe::default();

        for index in 1..=2 {
            coordinator
                .request_show_with(ShowTarget::Launcher, |coordinator, target| {
                    probe.trace.borrow_mut().push(format!("dispatch-{index}"));
                    let target = coordinator
                        .readiness
                        .lock()
                        .expect("readiness lock poisoned")
                        .request(target);
                    if let Some(target) = target {
                        run_show_case(&coordinator, target, index, ShowFailure::None, &probe)
                            .map_err(|_| ())?;
                    }
                    Ok(())
                })
                .unwrap();
        }

        assert_eq!(
            *probe.trace.borrow(),
            [
                "dispatch-1",
                "invocation-1",
                "center-1",
                "always-on-top-1",
                "show-1",
                "focus-1",
                "registry-on-show-1",
                "emit-1",
                "reserve-launcher-record-1",
                "dispatch-2",
                "invocation-2",
                "center-2",
                "always-on-top-2",
                "show-2",
                "focus-2",
                "registry-on-show-2",
                "emit-2",
                "reserve-launcher-record-2",
            ]
        );
        assert_eq!(probe.records.get(), 2);
        assert_eq!(probe.clears.get(), 0);
        assert_eq!(probe.payloads.borrow().len(), 2);
        assert_eq!(exit_snapshot(&coordinator).1, 0);
    }

    #[test]
    fn show_dispatch_failure_has_zero_side_effects() {
        let coordinator = coordinator_for_test();
        let dispatched = Cell::new(0);
        assert_eq!(
            coordinator.request_show_with(ShowTarget::Launcher, |_, _| {
                dispatched.set(dispatched.get() + 1);
                Err(())
            }),
            Err(LifecycleError::MainThreadDispatchFailed)
        );
        assert_eq!(dispatched.get(), 1);
        assert_eq!(coordinator.next_invocation.load(Ordering::Relaxed), 0);
        assert_eq!(
            coordinator
                .readiness
                .lock()
                .expect("readiness lock poisoned")
                .pending_target,
            None
        );
        assert_eq!(exit_snapshot(&coordinator).1, 0);
    }

    #[test]
    fn show_window_and_emit_failures_clear_once_while_center_continues() {
        for failure in [
            ShowFailure::AlwaysOnTop,
            ShowFailure::Show,
            ShowFailure::Focus,
            ShowFailure::Emit,
        ] {
            let coordinator = coordinator_for_test();
            let probe = ShowProbe::default();
            assert_eq!(
                run_show_case(&coordinator, ShowTarget::Launcher, 1, failure, &probe,),
                Err(LifecycleError::WindowFailed)
            );
            assert_eq!(probe.clears.get(), 1);
            assert_eq!(probe.records.get(), 0);
            assert_eq!(exit_snapshot(&coordinator).1, 0);
        }

        let coordinator = coordinator_for_test();
        let probe = ShowProbe::default();
        assert_eq!(
            run_show_case(
                &coordinator,
                ShowTarget::Launcher,
                1,
                ShowFailure::Center,
                &probe,
            ),
            Ok(ShowOutcome::Shown)
        );
        assert_eq!(probe.clears.get(), 0);
        assert_eq!(probe.records.get(), 1);

        let registry = ResultRegistry::default();
        registry.on_show("old".into());
        assert!(registry
            .begin_query(QueryDomain::Application, "old", 1)
            .is_some());
        let hides = Cell::new(0);
        let mut center = || Ok(());
        let mut always_on_top = || Ok(());
        let mut show = || Ok(());
        let mut focus = || Err(());
        let mut registry_on_show = |id| registry.on_show(id);
        let mut emit = |_: &LauncherShown| Ok(());
        let mut clear_and_hide = || {
            registry.hide_and_clear();
            hides.set(hides.get() + 1);
        };
        let mut record_launcher = |_: CriticalReservation| {};
        let mut operations = ShowMainClosures {
            center: &mut center,
            always_on_top: &mut always_on_top,
            show: &mut show,
            focus: &mut focus,
            registry_on_show: &mut registry_on_show,
            emit: &mut emit,
            clear_and_hide: &mut clear_and_hide,
            record_launcher: &mut record_launcher,
        };
        assert_eq!(
            coordinator_for_test().show_main_core(ShowTarget::Launcher, &mut operations),
            Err(LifecycleError::WindowFailed)
        );
        assert_eq!(hides.get(), 1);
        assert!(registry
            .begin_query(QueryDomain::Application, "old", 2)
            .is_none());
    }

    #[test]
    fn show_non_running_state_is_ignored_before_invocation() {
        for state in [
            ExitState::Cleaning,
            ExitState::Clean,
            ExitState::SystemEnding,
        ] {
            let coordinator = coordinator_for_test();
            coordinator
                .exit_gate
                .lock()
                .expect("exit gate lock poisoned")
                .state = state;
            let probe = ShowProbe::default();
            let resolutions = Cell::new(0);
            let observations = Cell::new(0);
            assert_eq!(
                coordinator.show_main_with_resolver(
                    || {
                        observations.set(observations.get() + 1);
                        coordinator.observe_exit()
                    },
                    || {
                        resolutions.set(resolutions.get() + 1);
                        run_show_case(
                            &coordinator,
                            ShowTarget::Launcher,
                            1,
                            ShowFailure::None,
                            &probe,
                        )
                    },
                ),
                Ok(None)
            );
            assert_eq!(observations.get(), 1);
            assert_eq!(resolutions.get(), 0);
            assert!(probe.trace.borrow().is_empty());
            assert_eq!(coordinator.next_invocation.load(Ordering::Relaxed), 0);
        }

        let coordinator = coordinator_for_test();
        let probe = ShowProbe::default();
        let observations = Cell::new(0);
        let resolutions = Cell::new(0);
        assert_eq!(
            coordinator.show_main_with_resolver(
                || {
                    observations.set(observations.get() + 1);
                    coordinator.observe_exit()
                },
                || {
                    resolutions.set(resolutions.get() + 1);
                    coordinator
                        .exit_gate
                        .lock()
                        .expect("exit gate lock poisoned")
                        .state = ExitState::Cleaning;
                    run_show_case(
                        &coordinator,
                        ShowTarget::Launcher,
                        1,
                        ShowFailure::None,
                        &probe,
                    )
                },
            ),
            Ok(Some(ShowOutcome::Shown))
        );
        assert_eq!(observations.get(), 1);
        assert_eq!(resolutions.get(), 1);
    }

    #[test]
    fn show_target_notice_and_record_failures_keep_first_pending_notice() {
        let coordinator = coordinator_for_test();
        coordinator.set_notice_once(LifecycleNotice::SettingsFailed);
        let probe = ShowProbe::default();
        assert_eq!(
            run_show_case(
                &coordinator,
                ShowTarget::Settings,
                1,
                ShowFailure::None,
                &probe,
            ),
            Ok(ShowOutcome::Shown)
        );
        assert_eq!(probe.records.get(), 0);
        assert_eq!(
            probe.payloads.borrow()[0].notice,
            Some(LifecycleNotice::SettingsFailed)
        );
        assert_eq!(*coordinator.pending_notice.lock().unwrap(), None);

        coordinator.set_notice_once(LifecycleNotice::SettingsFailed);
        assert_eq!(
            run_show_case(
                &coordinator,
                ShowTarget::Settings,
                2,
                ShowFailure::Emit,
                &probe,
            ),
            Err(LifecycleError::WindowFailed)
        );
        coordinator.set_notice_once(LifecycleNotice::ValidationFailed);
        assert_eq!(
            *coordinator.pending_notice.lock().unwrap(),
            Some(LifecycleNotice::SettingsFailed)
        );

        *coordinator.pending_notice.lock().unwrap() = None;
        assert_eq!(
            run_show_case(
                &coordinator,
                ShowTarget::Launcher,
                3,
                ShowFailure::Record,
                &probe,
            ),
            Ok(ShowOutcome::Shown)
        );
        assert_eq!(
            *coordinator.pending_notice.lock().unwrap(),
            Some(LifecycleNotice::ValidationFailed)
        );

        let panicking = coordinator_for_test();
        let panic_probe = ShowProbe::default();
        let panic_result = catch_unwind(AssertUnwindSafe(|| {
            let _ = run_show_case(
                &panicking,
                ShowTarget::Launcher,
                1,
                ShowFailure::PanicRecord,
                &panic_probe,
            );
        }));
        assert!(panic_result.is_err());
        assert_eq!(exit_snapshot(&panicking).1, 0);
    }

    #[test]
    fn show_invocation_overflow_and_payload_serialization_are_fixed() {
        let coordinator = coordinator_for_test();
        coordinator
            .next_invocation
            .store(u64::MAX, Ordering::Relaxed);
        let probe = ShowProbe::default();
        assert_eq!(
            run_show_case(
                &coordinator,
                ShowTarget::Launcher,
                1,
                ShowFailure::None,
                &probe,
            ),
            Err(LifecycleError::InvocationExhausted)
        );
        assert!(probe.trace.borrow().is_empty());
        assert_eq!(exit_snapshot(&coordinator).1, 0);

        for (target, expected) in [
            (ShowTarget::Launcher, "launcher"),
            (ShowTarget::Settings, "settings"),
        ] {
            let value = serde_json::to_value(LauncherShown {
                invocation_id: "invocation-1".into(),
                target,
                notice: Some(LifecycleNotice::ValidationFailed),
            })
            .unwrap();
            assert_eq!(value["target"], expected);
            assert_eq!(value["notice"], "validationFailed");
        }
    }

    fn coordinator_for_test() -> Arc<LifecycleCoordinator> {
        Arc::new(LifecycleCoordinator::default())
    }

    fn exit_snapshot(coordinator: &LifecycleCoordinator) -> (ExitState, usize, CleanAttempt) {
        let gate = coordinator
            .exit_gate
            .lock()
            .expect("exit gate lock poisoned");
        (gate.state, gate.in_flight_critical, gate.clean_attempt)
    }

    fn complete_requested_clean<F>(
        coordinator: &LifecycleCoordinator,
        decision: CleanDecision,
        marker: F,
    ) -> CleanDecision
    where
        F: FnOnce() -> CleanResult,
    {
        assert_eq!(decision, CleanDecision::CallMarker);
        coordinator.complete_clean(marker())
    }

    #[derive(Default)]
    struct AttemptTrace {
        next: u64,
        active: Option<u64>,
    }

    impl AttemptTrace {
        fn begin<F>(&mut self, coordinator: &LifecycleCoordinator, begin: F) -> (u64, CleanDecision)
        where
            F: FnOnce() -> CleanDecision,
        {
            let was_idle = matches!(exit_snapshot(coordinator).2, CleanAttempt::Idle);
            let decision = begin();
            let is_idle = matches!(exit_snapshot(coordinator).2, CleanAttempt::Idle);
            if was_idle && !is_idle {
                self.next += 1;
                self.active = Some(self.next);
            }
            (self.active.expect("attempt trace must be active"), decision)
        }

        fn observe(&self) -> u64 {
            self.active.expect("attempt trace must be active")
        }

        fn clear_if_idle(&mut self, coordinator: &LifecycleCoordinator) {
            if matches!(exit_snapshot(coordinator).2, CleanAttempt::Idle) {
                self.active = None;
            }
        }
    }

    #[test]
    fn critical_reservation_releases_once_on_drop() {
        let coordinator = coordinator_for_test();
        let reservation = coordinator.reserve_critical().unwrap();
        assert_eq!(exit_snapshot(&coordinator).1, 1);

        let waiting = Arc::clone(&coordinator);
        let barrier = Arc::new(Barrier::new(2));
        let waiting_barrier = Arc::clone(&barrier);
        let waiter = thread::spawn(move || {
            let gate = waiting.exit_gate.lock().expect("exit gate lock poisoned");
            waiting_barrier.wait();
            let (gate, timeout) = waiting
                .critical_changed
                .wait_timeout_while(gate, Duration::from_secs(1), |gate| {
                    gate.in_flight_critical != 0
                })
                .expect("exit gate lock poisoned");
            assert!(!timeout.timed_out());
            assert_eq!(gate.in_flight_critical, 0);
        });

        barrier.wait();
        drop(reservation);
        waiter.join().unwrap();
        assert_eq!(exit_snapshot(&coordinator).1, 0);
    }

    #[test]
    fn critical_reservation_rejects_non_running_states_and_overflow() {
        let coordinator = coordinator_for_test();
        for state in [
            ExitState::Cleaning,
            ExitState::Clean,
            ExitState::SystemEnding,
        ] {
            coordinator
                .exit_gate
                .lock()
                .expect("exit gate lock poisoned")
                .state = state;
            assert_eq!(
                coordinator.reserve_critical().unwrap_err(),
                ReservationError::NotRunning
            );
        }

        let mut gate = coordinator
            .exit_gate
            .lock()
            .expect("exit gate lock poisoned");
        gate.state = ExitState::Running;
        gate.in_flight_critical = usize::MAX;
        drop(gate);
        assert_eq!(
            coordinator.reserve_critical().unwrap_err(),
            ReservationError::Overflow
        );
        assert_eq!(exit_snapshot(&coordinator).1, usize::MAX);
    }

    #[test]
    fn critical_reservation_releases_on_error_and_panic() {
        fn fail_after_reserving(
            coordinator: &Arc<LifecycleCoordinator>,
        ) -> Result<(), &'static str> {
            let _reservation = coordinator.reserve_critical().unwrap();
            Err("sentinel")
        }

        let coordinator = coordinator_for_test();
        assert_eq!(fail_after_reserving(&coordinator), Err("sentinel"));
        assert_eq!(exit_snapshot(&coordinator).1, 0);

        let panicking = Arc::clone(&coordinator);
        let result = catch_unwind(AssertUnwindSafe(move || {
            let _reservation = panicking.reserve_critical().unwrap();
            panic!("reservation panic sentinel");
        }));
        assert!(result.is_err());
        assert_eq!(exit_snapshot(&coordinator).1, 0);
    }

    #[test]
    fn critical_plain_show_state_does_not_reserve() {
        let coordinator = coordinator_for_test();
        {
            let mut readiness = coordinator
                .readiness
                .lock()
                .expect("readiness lock poisoned");
            assert_eq!(readiness.request(ShowTarget::Launcher), None);
            assert_eq!(readiness.mark_setup_ready(), None);
            assert_eq!(readiness.mark_frontend_ready(), Some(ShowTarget::Launcher));
        }
        {
            let mut modal = coordinator.modal.lock().expect("modal lock poisoned");
            assert_eq!(modal.claim_export(), Ok(false));
            modal.finish_export();
            modal.on_successful_show();
        }
        assert_eq!(exit_snapshot(&coordinator).1, 0);
    }

    #[test]
    fn system_end_times_out_waiting_tray_without_waiting() {
        let coordinator = coordinator_for_test();
        let reservation = coordinator.reserve_critical().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        assert_eq!(
            coordinator.begin_tray_clean(deadline),
            CleanDecision::Wait { deadline }
        );
        assert_eq!(
            coordinator.begin_system_end_nonblocking(Instant::now()),
            CleanDecision::ObserveOnly
        );
        assert_eq!(
            exit_snapshot(&coordinator),
            (
                ExitState::SystemEnding,
                1,
                CleanAttempt::Finished(CleanResult::TimedOut)
            )
        );
        drop(reservation);
        assert_eq!(
            coordinator.advance_clean(Instant::now()),
            CleanDecision::ObserveOnly
        );
    }

    #[test]
    fn clean_attempt_tray_completion_cannot_revert_system_ending() {
        for result in [CleanResult::Succeeded, CleanResult::Failed] {
            let coordinator = coordinator_for_test();
            let deadline = Instant::now() + Duration::from_secs(5);
            assert_eq!(
                coordinator.begin_tray_clean(deadline),
                CleanDecision::CallMarker
            );
            assert_eq!(
                coordinator.begin_system_end_nonblocking(Instant::now()),
                CleanDecision::ObserveOnly
            );
            assert_eq!(
                coordinator.complete_clean(result),
                CleanDecision::ObserveOnly
            );
            assert_eq!(
                exit_snapshot(&coordinator),
                (ExitState::SystemEnding, 0, CleanAttempt::Finished(result))
            );
        }
    }

    #[test]
    fn clean_attempt_later_system_work_uses_a_fresh_trace_after_failure_or_timeout() {
        let marker_calls = Cell::new(0);

        let succeeded = coordinator_for_test();
        let success_deadline = Instant::now() + Duration::from_secs(5);
        let mut success_trace = AttemptTrace::default();
        let (tray_trace, tray) =
            success_trace.begin(&succeeded, || succeeded.begin_tray_clean(success_deadline));
        assert_eq!(
            complete_requested_clean(&succeeded, tray, || {
                marker_calls.set(marker_calls.get() + 1);
                CleanResult::Succeeded
            }),
            CleanDecision::Exit
        );
        assert_eq!(
            succeeded.begin_system_end_nonblocking(Instant::now()),
            CleanDecision::ObserveOnly
        );
        assert_eq!(success_trace.observe(), tray_trace);
        assert_eq!(marker_calls.get(), 1);

        marker_calls.set(0);
        let failed = coordinator_for_test();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut failed_trace = AttemptTrace::default();
        let (tray_trace, tray) = failed_trace.begin(&failed, || failed.begin_tray_clean(deadline));
        assert_eq!(
            complete_requested_clean(&failed, tray, || {
                marker_calls.set(marker_calls.get() + 1);
                CleanResult::Failed
            }),
            CleanDecision::ReturnRunning
        );
        failed_trace.clear_if_idle(&failed);
        let (system_trace, system) = failed_trace.begin(&failed, || {
            failed.begin_system_end_nonblocking(Instant::now())
        });
        assert_ne!(tray_trace, system_trace);
        assert_eq!(
            complete_requested_clean(&failed, system, || {
                marker_calls.set(marker_calls.get() + 1);
                CleanResult::Succeeded
            }),
            CleanDecision::ObserveOnly
        );
        assert_eq!(marker_calls.get(), 2);

        marker_calls.set(0);
        let timed_out = coordinator_for_test();
        let reservation = timed_out.reserve_critical().unwrap();
        let timeout_deadline = Instant::now() + Duration::from_secs(1);
        let mut timeout_trace = AttemptTrace::default();
        let (tray_trace, tray) =
            timeout_trace.begin(&timed_out, || timed_out.begin_tray_clean(timeout_deadline));
        assert_eq!(
            tray,
            CleanDecision::Wait {
                deadline: timeout_deadline
            }
        );
        assert_eq!(
            timed_out.advance_clean(timeout_deadline),
            CleanDecision::ReturnRunning
        );
        timeout_trace.clear_if_idle(&timed_out);
        drop(reservation);
        let (system_trace, system) = timeout_trace.begin(&timed_out, || {
            timed_out.begin_system_end_nonblocking(Instant::now())
        });
        assert_ne!(tray_trace, system_trace);
        assert_eq!(
            complete_requested_clean(&timed_out, system, || {
                marker_calls.set(marker_calls.get() + 1);
                CleanResult::Succeeded
            }),
            CleanDecision::ObserveOnly
        );
        assert_eq!(marker_calls.get(), 1);
    }

    #[test]
    fn clean_attempt_timeouts_and_exit_observer_are_fail_closed() {
        let tray = coordinator_for_test();
        let tray_reservation = tray.reserve_critical().unwrap();
        let tray_deadline = Instant::now() + Duration::from_secs(1);
        assert_eq!(
            tray.begin_tray_clean(tray_deadline),
            CleanDecision::Wait {
                deadline: tray_deadline
            }
        );
        assert_eq!(
            tray.advance_clean(tray_deadline),
            CleanDecision::ReturnRunning
        );
        assert_eq!(
            exit_snapshot(&tray),
            (ExitState::Running, 1, CleanAttempt::Idle)
        );
        drop(tray_reservation);

        let late_marker_calls = Cell::new(0);
        let released_tray = coordinator_for_test();
        let released_tray_reservation = released_tray.reserve_critical().unwrap();
        let released_tray_deadline = Instant::now();
        assert_eq!(
            released_tray.begin_tray_clean(released_tray_deadline),
            CleanDecision::Wait {
                deadline: released_tray_deadline
            }
        );
        drop(released_tray_reservation);
        let released_tray_decision = released_tray.advance_clean(released_tray_deadline);
        if released_tray_decision == CleanDecision::CallMarker {
            late_marker_calls.set(late_marker_calls.get() + 1);
        }
        assert_eq!(released_tray_decision, CleanDecision::ReturnRunning);
        assert_eq!(late_marker_calls.get(), 0);
        assert_eq!(
            exit_snapshot(&released_tray),
            (ExitState::Running, 0, CleanAttempt::Idle)
        );

        let shared = coordinator_for_test();
        let shared_reservation = shared.reserve_critical().unwrap();
        let shared_deadline = Instant::now() + Duration::from_secs(1);
        assert_eq!(
            shared.begin_tray_clean(shared_deadline),
            CleanDecision::Wait {
                deadline: shared_deadline
            }
        );
        assert_eq!(
            shared.begin_system_end_nonblocking(Instant::now()),
            CleanDecision::ObserveOnly
        );
        assert_eq!(
            exit_snapshot(&shared),
            (
                ExitState::SystemEnding,
                1,
                CleanAttempt::Finished(CleanResult::TimedOut)
            )
        );
        drop(shared_reservation);

        let released_shared = coordinator_for_test();
        let released_shared_reservation = released_shared.reserve_critical().unwrap();
        let released_shared_deadline = Instant::now();
        assert_eq!(
            released_shared.begin_tray_clean(released_shared_deadline),
            CleanDecision::Wait {
                deadline: released_shared_deadline
            }
        );
        assert_eq!(
            released_shared.begin_system_end_nonblocking(Instant::now()),
            CleanDecision::ObserveOnly
        );
        drop(released_shared_reservation);
        let released_shared_decision = released_shared.advance_clean(released_shared_deadline);
        if released_shared_decision == CleanDecision::CallMarker {
            late_marker_calls.set(late_marker_calls.get() + 1);
        }
        assert_eq!(released_shared_decision, CleanDecision::ObserveOnly);
        assert_eq!(late_marker_calls.get(), 0);
        assert_eq!(
            exit_snapshot(&released_shared),
            (
                ExitState::SystemEnding,
                0,
                CleanAttempt::Finished(CleanResult::TimedOut)
            )
        );

        let system = coordinator_for_test();
        let system_reservation = system.reserve_critical().unwrap();
        let system_deadline = Instant::now() + Duration::from_secs(1);
        assert_eq!(
            system.begin_system_end_nonblocking(system_deadline),
            CleanDecision::ObserveOnly
        );
        assert_eq!(
            exit_snapshot(&system),
            (
                ExitState::SystemEnding,
                1,
                CleanAttempt::Finished(CleanResult::TimedOut)
            )
        );
        drop(system_reservation);
        assert_eq!(
            system.advance_clean(Instant::now()),
            CleanDecision::ObserveOnly
        );
        assert_eq!(
            system.begin_tray_clean(Instant::now()),
            CleanDecision::ObserveOnly
        );
        assert_eq!(system.observe_exit(), ExitState::SystemEnding);
    }

    #[test]
    fn runtime_settings_startup_reconciles_persisted_values_or_sets_first_notice() {
        let coordinator = coordinator_for_test();
        let shortcut: Shortcut = "Alt+Space".parse().unwrap();
        let trace = RefCell::new(Vec::new());
        coordinator
            .reconcile_runtime_settings_with(
                "Alt+Space",
                true,
                (
                    HotkeyKind::parse,
                    |registered| {
                        assert_eq!(registered, shortcut);
                        trace.borrow_mut().push("register");
                        Ok(())
                    },
                    |_| Ok(()),
                ),
                (
                    || {
                        trace.borrow_mut().push("read-autostart");
                        Ok(false)
                    },
                    |enabled| {
                        assert!(enabled);
                        trace.borrow_mut().push("autostart-true");
                        Ok(())
                    },
                ),
            )
            .unwrap();
        assert_eq!(
            coordinator
                .runtime_settings
                .lock()
                .expect("runtime settings lock poisoned")
                .registered,
            [shortcut]
        );
        assert_eq!(
            *trace.borrow(),
            ["register", "read-autostart", "autostart-true"]
        );
        assert_eq!(*coordinator.pending_notice.lock().unwrap(), None);

        let matching = coordinator_for_test();
        let matching_registers = Cell::new(0);
        let matching_reads = Cell::new(0);
        let matching_changes = Cell::new(0);
        matching
            .reconcile_runtime_settings_with(
                "Alt+Space",
                false,
                (
                    HotkeyKind::parse,
                    |registered| {
                        assert_eq!(registered, shortcut);
                        matching_registers.set(matching_registers.get() + 1);
                        Ok(())
                    },
                    |_| Ok(()),
                ),
                (
                    || {
                        matching_reads.set(matching_reads.get() + 1);
                        Ok(false)
                    },
                    |_| {
                        matching_changes.set(matching_changes.get() + 1);
                        Ok(())
                    },
                ),
            )
            .unwrap();
        assert_eq!(matching_registers.get(), 1);
        assert_eq!(matching_reads.get(), 1);
        assert_eq!(matching_changes.get(), 0);
        assert_eq!(
            matching
                .runtime_settings
                .lock()
                .expect("runtime settings lock poisoned")
                .registered,
            [shortcut]
        );

        for register_fails in [false, true] {
            let coordinator = coordinator_for_test();
            let result = coordinator.reconcile_runtime_settings_with(
                if register_fails {
                    "Alt+Space"
                } else {
                    "not a shortcut"
                },
                false,
                (
                    HotkeyKind::parse,
                    |_| if register_fails { Err(()) } else { Ok(()) },
                    |_| Ok(()),
                ),
                (
                    || panic!("failed shortcut setup must skip autostart read"),
                    |_| panic!("failed shortcut setup must skip autostart change"),
                ),
            );
            assert_eq!(result, Err(()));
            assert!(coordinator
                .runtime_settings
                .lock()
                .expect("runtime settings lock poisoned")
                .registered
                .is_empty());
            assert_eq!(
                *coordinator.pending_notice.lock().unwrap(),
                Some(LifecycleNotice::SettingsFailed)
            );
        }

        let read_failure = coordinator_for_test();
        read_failure.set_notice_once(LifecycleNotice::ValidationFailed);
        let read_failure_registers = Cell::new(0);
        let read_failure_changes = Cell::new(0);
        assert_eq!(
            read_failure.reconcile_runtime_settings_with(
                "Alt+Space",
                false,
                (
                    HotkeyKind::parse,
                    |registered| {
                        assert_eq!(registered, shortcut);
                        read_failure_registers.set(read_failure_registers.get() + 1);
                        Ok(())
                    },
                    |_| Ok(()),
                ),
                (
                    || Err(()),
                    |_| {
                        read_failure_changes.set(read_failure_changes.get() + 1);
                        Ok(())
                    },
                ),
            ),
            Err(())
        );
        assert_eq!(read_failure_registers.get(), 1);
        assert_eq!(read_failure_changes.get(), 0);
        assert_eq!(
            read_failure
                .runtime_settings
                .lock()
                .expect("runtime settings lock poisoned")
                .registered,
            [shortcut]
        );
        assert_eq!(
            *read_failure.pending_notice.lock().unwrap(),
            Some(LifecycleNotice::ValidationFailed),
            "startup SettingsFailed must not overwrite the first notice"
        );

        let change_failure = coordinator_for_test();
        let change_failure_registers = Cell::new(0);
        let change_calls = Cell::new(0);
        assert_eq!(
            change_failure.reconcile_runtime_settings_with(
                "Alt+Space",
                true,
                (
                    HotkeyKind::parse,
                    |registered| {
                        assert_eq!(registered, shortcut);
                        change_failure_registers.set(change_failure_registers.get() + 1);
                        Ok(())
                    },
                    |_| Ok(()),
                ),
                (
                    || Ok(false),
                    |_| {
                        change_calls.set(change_calls.get() + 1);
                        Err(())
                    },
                ),
            ),
            Err(())
        );
        assert_eq!(change_failure_registers.get(), 1);
        assert_eq!(change_calls.get(), 1);
        assert_eq!(
            change_failure
                .runtime_settings
                .lock()
                .expect("runtime settings lock poisoned")
                .registered,
            [shortcut]
        );
        assert_eq!(
            *change_failure.pending_notice.lock().unwrap(),
            Some(LifecycleNotice::SettingsFailed)
        );
    }

    #[test]
    fn save_blocks_clean_at_every_runtime_transaction_phase() {
        #[derive(Default)]
        struct RejectedSaveCounts {
            dispatch: AtomicUsize,
            register: AtomicUsize,
            unregister: AtomicUsize,
            autostart: AtomicUsize,
            persist: AtomicUsize,
            store: AtomicUsize,
        }

        impl RejectedSaveCounts {
            fn assert_zero(&self) {
                assert_eq!(self.dispatch.load(Ordering::Relaxed), 0);
                assert_eq!(self.register.load(Ordering::Relaxed), 0);
                assert_eq!(self.unregister.load(Ordering::Relaxed), 0);
                assert_eq!(self.autostart.load(Ordering::Relaxed), 0);
                assert_eq!(self.persist.load(Ordering::Relaxed), 0);
                assert_eq!(self.store.load(Ordering::Relaxed), 0);
            }
        }

        fn assert_clean_blocked(coordinator: &LifecycleCoordinator) {
            assert_eq!(exit_snapshot(coordinator).1, 1);
            let deadline = Instant::now();
            assert_eq!(
                coordinator.begin_tray_clean(deadline),
                CleanDecision::Wait { deadline }
            );
            assert_eq!(
                coordinator.begin_system_end_nonblocking(Instant::now()),
                CleanDecision::ObserveOnly
            );
            assert!(matches!(
                exit_snapshot(coordinator),
                (
                    ExitState::SystemEnding,
                    1,
                    CleanAttempt::Finished(CleanResult::TimedOut)
                )
            ));
        }

        for phase in [
            "stale-cleanup",
            "register",
            "unregister",
            "autostart",
            "persist",
            "autostart-rollback",
            "rollback-unregister",
        ] {
            let coordinator = coordinator_for_test();
            let reserve_coordinator = Arc::clone(&coordinator);
            let worker_coordinator = Arc::clone(&coordinator);
            let stale: Shortcut = "Shift+Space".parse().unwrap();
            let old: Shortcut = "Alt+Space".parse().unwrap();
            let requested: Shortcut = "Ctrl+Space".parse().unwrap();
            let mut state = RuntimeSettings {
                registered: if phase == "stale-cleanup" {
                    vec![stale, old]
                } else {
                    vec![old]
                },
                installed_hook: None,
            };
            let autostart_calls = Cell::new(0);
            let persist_fails = matches!(phase, "autostart-rollback" | "rollback-unregister");
            let result = tauri::async_runtime::block_on(save_settings_worker_with(
                move || reserve_coordinator.reserve_critical().map_err(|_| ()),
                move |reservation| {
                    let _reservation = reservation;
                    state.apply_hotkey_binding(
                        HotkeyBindingChange {
                            persisted: HotkeyKind::Chord(old),
                            requested: HotkeyKind::Chord(requested),
                            autostart: true,
                        },
                        (
                            |_| {
                                if phase == "register" {
                                    assert_clean_blocked(&worker_coordinator);
                                }
                                Ok(())
                            },
                            |shortcut| {
                                if (phase == "stale-cleanup" && shortcut == stale)
                                    || (phase == "unregister" && shortcut == old)
                                    || (phase == "rollback-unregister" && shortcut == requested)
                                {
                                    assert_clean_blocked(&worker_coordinator);
                                }
                                Ok(())
                            },
                        ),
                        (
                            |_| panic!("chord transaction must not install a hook"),
                            || panic!("chord transaction must not uninstall a hook"),
                        ),
                        (
                            || Ok(false),
                            |_| {
                                let call = autostart_calls.get() + 1;
                                autostart_calls.set(call);
                                if (phase == "autostart" && call == 1)
                                    || (phase == "autostart-rollback" && call == 2)
                                {
                                    assert_clean_blocked(&worker_coordinator);
                                }
                                Ok(())
                            },
                        ),
                        || {
                            if phase == "persist" {
                                assert_clean_blocked(&worker_coordinator);
                            }
                            if persist_fails {
                                Err(())
                            } else {
                                Ok(())
                            }
                        },
                    )
                },
            ));
            assert_eq!(result.is_err(), persist_fails);
        }

        for state in [ExitState::Cleaning, ExitState::SystemEnding] {
            let coordinator = coordinator_for_test();
            coordinator
                .exit_gate
                .lock()
                .expect("exit gate lock poisoned")
                .state = state;
            let reserve_coordinator = Arc::clone(&coordinator);
            let counts = Arc::new(RejectedSaveCounts::default());
            let worker_counts = Arc::clone(&counts);
            let result = tauri::async_runtime::block_on(save_settings_worker_with(
                move || reserve_coordinator.reserve_critical().map_err(|_| ()),
                move |_reservation| {
                    worker_counts.dispatch.fetch_add(1, Ordering::Relaxed);
                    worker_counts.register.fetch_add(1, Ordering::Relaxed);
                    worker_counts.unregister.fetch_add(1, Ordering::Relaxed);
                    worker_counts.autostart.fetch_add(1, Ordering::Relaxed);
                    worker_counts.persist.fetch_add(1, Ordering::Relaxed);
                    worker_counts.store.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                },
            ));
            assert!(result.is_err());
            counts.assert_zero();
            assert_eq!(exit_snapshot(&coordinator).1, 0);
        }
    }

    #[test]
    fn production_callbacks_focus_query_and_hide_seams_are_lock_free() {
        let coordinator = coordinator_for_test();
        let clear_calls = Cell::new(0);
        coordinator
            .handle_focus_event_with(
                false,
                || panic!("Normal focus loss must not query focus"),
                || {
                    assert!(coordinator.modal.try_lock().is_ok());
                    clear_calls.set(clear_calls.get() + 1);
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(clear_calls.get(), 1);

        let guard = coordinator.begin_modal_export(|| Ok(true)).unwrap();
        coordinator
            .handle_focus_event_with(
                false,
                || panic!("Open must not query focus"),
                || panic!("Open must suppress hide"),
            )
            .unwrap();
        assert_eq!(*coordinator.modal.lock().unwrap(), ModalState::Open);
        drop(guard);
    }

    #[test]
    fn production_callbacks_focus_awaiting_resolves_every_query_result() {
        for (focus_result, expected, expected_clears) in [
            (Ok(true), Ok(()), 0),
            (Ok(false), Ok(()), 1),
            (Err(()), Err(LifecycleError::WindowFailed), 1),
        ] {
            let coordinator = coordinator_for_test();
            drop(coordinator.begin_modal_export(|| Ok(true)).unwrap());
            let clear_calls = Cell::new(0);
            assert_eq!(
                coordinator.handle_focus_event_with(
                    false,
                    || {
                        assert!(coordinator.modal.try_lock().is_ok());
                        focus_result
                    },
                    || {
                        assert!(coordinator.modal.try_lock().is_ok());
                        clear_calls.set(clear_calls.get() + 1);
                        Ok(())
                    },
                ),
                expected
            );
            assert_eq!(clear_calls.get(), expected_clears);
            assert_eq!(*coordinator.modal.lock().unwrap(), ModalState::Normal);
        }
    }

    #[test]
    fn production_callbacks_focus_repeated_events_never_leave_awaiting() {
        let coordinator = coordinator_for_test();
        drop(coordinator.begin_modal_export(|| Ok(true)).unwrap());
        coordinator
            .handle_focus_event_with(false, || Ok(true), || panic!("still focused"))
            .unwrap();
        assert_eq!(*coordinator.modal.lock().unwrap(), ModalState::Normal);

        let clear_calls = Cell::new(0);
        coordinator
            .handle_focus_event_with(
                false,
                || panic!("late Normal focus loss must not query"),
                || {
                    clear_calls.set(clear_calls.get() + 1);
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(clear_calls.get(), 1);

        let second = coordinator.begin_modal_export(|| panic!("Normal retry must not query"));
        assert!(second.is_ok());
        drop(second);
        coordinator
            .handle_focus_event_with(
                true,
                || panic!("Focused(true) must not query"),
                || panic!("Focused(true) must not hide"),
            )
            .unwrap();
        assert_eq!(*coordinator.modal.lock().unwrap(), ModalState::Normal);
    }

    #[test]
    fn tray_accepts_only_exact_namespaced_ids() {
        assert_eq!(
            tray_action(TRAY_OPEN_LAUNCHER),
            Some(TrayAction::Show(ShowTarget::Launcher))
        );
        assert_eq!(
            tray_action(TRAY_OPEN_SETTINGS),
            Some(TrayAction::Show(ShowTarget::Settings))
        );
        assert_eq!(tray_action(TRAY_QUIT), Some(TrayAction::Quit));
        for rejected in [
            "open-settings",
            "open-launcher",
            "quit",
            "uipilot.tray.open",
            "UIPILOT.TRAY.QUIT",
            "uipilot.tray.quit ",
            "uipilot.tray.open-launcher ",
            "",
        ] {
            assert_eq!(tray_action(rejected), None);
        }
    }

    #[test]
    fn tray_quit_starts_one_attempt_and_exits_only_after_marker_success() {
        let coordinator = coordinator_for_test();
        let deadline = Instant::now() + Duration::from_secs(1);
        let first = coordinator.begin_tray_clean(deadline);
        assert_eq!(first, CleanDecision::CallMarker);
        let repeated = coordinator.begin_tray_clean(deadline);
        assert_eq!(repeated, CleanDecision::ObserveOnly);

        let marker_calls = Cell::new(0);
        let exit_calls = Cell::new(0);
        let show_calls = Cell::new(0);
        assert_eq!(
            coordinator.run_tray_quit_with(
                repeated,
                |_| panic!("repeated quit must not wait"),
                || {
                    marker_calls.set(marker_calls.get() + 1);
                    CleanResult::Succeeded
                },
                || exit_calls.set(exit_calls.get() + 1),
                |_| show_calls.set(show_calls.get() + 1),
            ),
            CleanDecision::ObserveOnly
        );
        assert_eq!(marker_calls.get(), 0);

        assert_eq!(
            coordinator.run_tray_quit_with(
                first,
                |_| panic!("immediate marker call must not wait"),
                || {
                    marker_calls.set(marker_calls.get() + 1);
                    CleanResult::Succeeded
                },
                || {
                    assert_eq!(exit_snapshot(&coordinator).0, ExitState::Clean);
                    exit_calls.set(exit_calls.get() + 1);
                },
                |_| show_calls.set(show_calls.get() + 1),
            ),
            CleanDecision::Exit
        );
        assert_eq!(marker_calls.get(), 1);
        assert_eq!(exit_calls.get(), 1);
        assert_eq!(show_calls.get(), 0);
        assert_eq!(exit_snapshot(&coordinator).0, ExitState::Clean);
    }

    #[test]
    fn tray_worker_handles_timeout_failure_and_system_takeover_without_extra_ui() {
        let timed_out = coordinator_for_test();
        let reservation = timed_out.reserve_critical().unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        let initial = timed_out.begin_tray_clean(deadline);
        let marker_calls = Cell::new(0);
        let exit_calls = Cell::new(0);
        let shown = RefCell::new(Vec::new());
        assert_eq!(
            timed_out.run_tray_quit_with(
                initial,
                |_| deadline,
                || {
                    marker_calls.set(marker_calls.get() + 1);
                    CleanResult::Succeeded
                },
                || exit_calls.set(exit_calls.get() + 1),
                |target| shown.borrow_mut().push(target),
            ),
            CleanDecision::ReturnRunning
        );
        assert_eq!(marker_calls.get(), 0);
        assert_eq!(exit_calls.get(), 0);
        assert_eq!(*shown.borrow(), [ShowTarget::Settings]);
        assert_eq!(
            *timed_out.pending_notice.lock().unwrap(),
            Some(LifecycleNotice::ValidationFailed)
        );
        assert_eq!(exit_snapshot(&timed_out).0, ExitState::Running);
        drop(reservation);

        let failed = coordinator_for_test();
        let shown = Cell::new(0);
        assert_eq!(
            failed.run_tray_quit_with(
                failed.begin_tray_clean(deadline),
                |_| panic!("immediate marker call must not wait"),
                || CleanResult::Failed,
                || panic!("failed cleanup must not exit"),
                |_| shown.set(shown.get() + 1),
            ),
            CleanDecision::ReturnRunning
        );
        assert_eq!(shown.get(), 1);
        assert_eq!(exit_snapshot(&failed).0, ExitState::Running);

        let shared = coordinator_for_test();
        let tray = shared.begin_tray_clean(deadline);
        assert_eq!(tray, CleanDecision::CallMarker);
        assert_eq!(
            shared.begin_system_end_nonblocking(Instant::now()),
            CleanDecision::ObserveOnly
        );
        let marker_calls = Cell::new(0);
        assert_eq!(
            shared.run_tray_quit_with(
                tray,
                |_| panic!("tray owns the marker call"),
                || {
                    marker_calls.set(marker_calls.get() + 1);
                    CleanResult::Succeeded
                },
                || panic!("SystemEnding must not request app exit"),
                |_| panic!("SystemEnding must not show UI"),
            ),
            CleanDecision::ObserveOnly
        );
        assert_eq!(marker_calls.get(), 1);
        assert_eq!(exit_snapshot(&shared).0, ExitState::SystemEnding);
    }

    #[test]
    fn session_messages_query_false_and_other_continue_default_chain_once() {
        use windows::Win32::UI::WindowsAndMessaging::{WM_ENDSESSION, WM_QUERYENDSESSION};

        for (message, wparam, expected) in [
            (WM_QUERYENDSESSION, 1, 71),
            (WM_ENDSESSION, 0, 0),
            (0x0400, 0, 73),
        ] {
            let default_calls = Cell::new(0);
            let clean_calls = Cell::new(0);
            let destroy_calls = Cell::new(0);
            let result = handle_session_message_with(
                message,
                wparam,
                || {
                    default_calls.set(default_calls.get() + 1);
                    if message == WM_QUERYENDSESSION {
                        71
                    } else {
                        73
                    }
                },
                || clean_calls.set(clean_calls.get() + 1),
                || destroy_calls.set(destroy_calls.get() + 1),
            );
            assert_eq!(result, expected);
            assert_eq!(default_calls.get(), 1);
            assert_eq!(clean_calls.get(), 0);
            assert_eq!(destroy_calls.get(), 0);
        }
    }

    #[test]
    fn session_messages_true_runs_one_system_attempt_and_never_repeats_success() {
        use windows::Win32::UI::WindowsAndMessaging::WM_ENDSESSION;

        let coordinator = coordinator_for_test();
        let deadline = Instant::now() + Duration::from_secs(1);
        let marker_calls = Cell::new(0);
        let default_calls = Cell::new(0);
        assert_eq!(
            handle_session_message_with(
                WM_ENDSESSION,
                1,
                || {
                    default_calls.set(default_calls.get() + 1);
                    77
                },
                || {
                    assert_eq!(
                        coordinator.run_system_end_nonblocking_with(
                            deadline,
                            || {
                                marker_calls.set(marker_calls.get() + 1);
                                CleanResult::Succeeded
                            },
                            || {}
                        ),
                        CleanDecision::ObserveOnly
                    );
                },
                || panic!("WM_ENDSESSION must not destroy the subclass"),
            ),
            0
        );
        assert_eq!(marker_calls.get(), 1);
        assert_eq!(default_calls.get(), 1);
        assert!(matches!(
            exit_snapshot(&coordinator),
            (
                ExitState::SystemEnding,
                0,
                CleanAttempt::Finished(CleanResult::Succeeded)
            )
        ));

        assert_eq!(
            coordinator.run_system_end_nonblocking_with(
                deadline,
                || {
                    marker_calls.set(marker_calls.get() + 1);
                    CleanResult::Succeeded
                },
                || {}
            ),
            CleanDecision::ObserveOnly
        );
        assert_eq!(marker_calls.get(), 1);

        for first_result in [CleanResult::Failed, CleanResult::TimedOut] {
            let coordinator = coordinator_for_test();
            let reservation = (first_result == CleanResult::TimedOut)
                .then(|| coordinator.reserve_critical().unwrap());
            let tray = coordinator.begin_tray_clean(deadline);
            let first_marker_calls = Cell::new(0);
            let first = if first_result == CleanResult::TimedOut {
                coordinator.run_tray_quit_with(
                    tray,
                    |_| deadline,
                    || {
                        first_marker_calls.set(first_marker_calls.get() + 1);
                        CleanResult::Succeeded
                    },
                    || panic!("timed out tray attempt must not exit"),
                    |_| {},
                )
            } else {
                complete_requested_clean(&coordinator, tray, || {
                    first_marker_calls.set(first_marker_calls.get() + 1);
                    first_result
                })
            };
            assert_eq!(first, CleanDecision::ReturnRunning);
            drop(reservation);
            let system_marker_calls = Cell::new(0);
            assert_eq!(
                coordinator.run_system_end_nonblocking_with(
                    deadline,
                    || {
                        system_marker_calls.set(system_marker_calls.get() + 1);
                        CleanResult::Succeeded
                    },
                    || {}
                ),
                CleanDecision::ObserveOnly
            );
            assert_eq!(system_marker_calls.get(), 1);
            assert_eq!(
                first_marker_calls.get() + system_marker_calls.get(),
                if first_result == CleanResult::Failed {
                    2
                } else {
                    1
                }
            );
        }
    }

    #[test]
    fn session_messages_exit_and_close_decisions_are_fail_closed() {
        let coordinator = coordinator_for_test();
        for (state, prevent) in [
            (ExitState::Running, true),
            (ExitState::Cleaning, true),
            (ExitState::Clean, false),
            (ExitState::SystemEnding, false),
        ] {
            coordinator.exit_gate.lock().unwrap().state = state;
            assert_eq!(coordinator.should_prevent_exit(), prevent);
            assert_eq!(coordinator.should_prevent_close(), prevent);
            coordinator.observe_run_exit();
        }
    }

    #[test]
    fn subclass_ownership_reclaims_context_once_on_install_failure_and_destroy() {
        use windows::Win32::UI::WindowsAndMessaging::WM_NCDESTROY;

        struct DropProbe(Arc<AtomicUsize>);
        impl Drop for DropProbe {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }

        let failed_drops = Arc::new(AtomicUsize::new(0));
        assert!(
            install_subclass_context_with(DropProbe(Arc::clone(&failed_drops)), |_| false).is_err()
        );
        assert_eq!(failed_drops.load(Ordering::Relaxed), 1);

        let drops = Arc::new(AtomicUsize::new(0));
        let raw = install_subclass_context_with(DropProbe(Arc::clone(&drops)), |_| true).unwrap();
        let remove_calls = Cell::new(0);
        let default_calls = Cell::new(0);
        assert_eq!(
            handle_session_message_with(
                WM_NCDESTROY,
                0,
                || {
                    default_calls.set(default_calls.get() + 1);
                    79
                },
                || panic!("destroy must not clean the marker"),
                || {
                    remove_calls.set(remove_calls.get() + 1);
                    assert!(unsafe { remove_subclass_context_with::<DropProbe, _>(raw, || true) });
                },
            ),
            79
        );
        assert_eq!(remove_calls.get(), 1);
        assert_eq!(default_calls.get(), 1);
        assert_eq!(drops.load(Ordering::Relaxed), 1);

        let retained_drops = Arc::new(AtomicUsize::new(0));
        let retained_raw =
            install_subclass_context_with(DropProbe(Arc::clone(&retained_drops)), |_| true)
                .unwrap();
        let remove_calls = Cell::new(0);
        let default_calls = Cell::new(0);
        assert_eq!(
            handle_session_message_with(
                WM_NCDESTROY,
                0,
                || {
                    default_calls.set(default_calls.get() + 1);
                    assert_eq!(retained_drops.load(Ordering::Relaxed), 0);
                    83
                },
                || panic!("destroy must not clean the marker"),
                || {
                    remove_calls.set(remove_calls.get() + 1);
                    assert!(!unsafe {
                        remove_subclass_context_with::<DropProbe, _>(retained_raw, || false)
                    });
                },
            ),
            83
        );
        assert_eq!(remove_calls.get(), 1);
        assert_eq!(default_calls.get(), 1);
        assert_eq!(retained_drops.load(Ordering::Relaxed), 0);
        unsafe { reclaim_subclass_context::<DropProbe>(retained_raw) };
        assert_eq!(retained_drops.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn subclass_ownership_uses_generated_windows_abi_and_app_handle_only() {
        use windows::Win32::UI::Shell::SUBCLASSPROC;

        const _: SUBCLASSPROC = Some(session_subclass_proc);
        const _: fn(&AppHandle, &tauri::WebviewWindow) -> Result<(), LifecycleError> =
            install_session_end_hook;

        let source = include_str!("lifecycle.rs").replace("\r\n", "\n");
        let marker = ["#[cfg(", "test", ")]\nmod tests"].concat();
        let production = source.split(&marker).next().unwrap();
        for generated_api in [
            "SetWindowSubclass",
            "RemoveWindowSubclass",
            "DefSubclassProc",
        ] {
            assert!(production.contains(generated_api));
        }
        assert!(production.contains("install_subclass_context_with(app.clone()"));
        assert!(!production.contains("windows_link::link!"));
        assert!(!production.contains("#[link("));
        assert!(!production.contains("struct SessionHookContext"));
    }

    struct HotkeyBindingProbe {
        trace: RefCell<Vec<String>>,
        autostart: Cell<Result<bool, ()>>,
        failures: Vec<(String, usize)>,
        actual_registered: RefCell<Vec<Shortcut>>,
        actual_hook: Cell<Option<DoubleTapModifier>>,
    }

    impl Default for HotkeyBindingProbe {
        fn default() -> Self {
            Self {
                trace: RefCell::new(Vec::new()),
                autostart: Cell::new(Ok(false)),
                failures: Vec::new(),
                actual_registered: RefCell::new(Vec::new()),
                actual_hook: Cell::new(None),
            }
        }
    }

    impl HotkeyBindingProbe {
        fn from_runtime(runtime: &RuntimeSettings, failures: Vec<(String, usize)>) -> Self {
            Self {
                failures,
                actual_registered: RefCell::new(runtime.registered.clone()),
                actual_hook: Cell::new(runtime.installed_hook),
                ..Self::default()
            }
        }

        fn record(&self, operation: String) -> Result<(), ()> {
            let call = self
                .trace
                .borrow()
                .iter()
                .filter(|recorded| **recorded == operation)
                .count()
                + 1;
            self.trace.borrow_mut().push(operation.clone());
            (!self.failures.contains(&(operation, call)))
                .then_some(())
                .ok_or(())
        }

        fn register(&self, shortcut: Shortcut) -> Result<(), ()> {
            self.record(format!("register-{shortcut}"))?;
            if !self.actual_registered.borrow().contains(&shortcut) {
                self.actual_registered.borrow_mut().push(shortcut);
            }
            Ok(())
        }

        fn unregister(&self, shortcut: Shortcut) -> Result<(), ()> {
            self.record(format!("unregister-{shortcut}"))?;
            self.actual_registered
                .borrow_mut()
                .retain(|registered| *registered != shortcut);
            Ok(())
        }

        fn install(&self, modifier: DoubleTapModifier) -> Result<(), ()> {
            self.record(format!("install-{modifier:?}"))?;
            self.actual_hook.set(Some(modifier));
            Ok(())
        }

        fn uninstall(&self) -> Result<(), ()> {
            self.record("uninstall-hook".into())?;
            self.actual_hook.set(None);
            Ok(())
        }

        fn read_autostart(&self) -> Result<bool, ()> {
            self.record("read-autostart".into())?;
            self.autostart.get()
        }

        fn change_autostart(&self, enabled: bool) -> Result<(), ()> {
            self.record(format!("autostart-{enabled}"))?;
            self.autostart.set(Ok(enabled));
            Ok(())
        }

        fn persist(&self) -> Result<(), ()> {
            self.record("persist".into())
        }

        fn assert_actual_matches(&self, runtime: &RuntimeSettings) {
            assert_eq!(*self.actual_registered.borrow(), runtime.registered);
            assert_eq!(self.actual_hook.get(), runtime.installed_hook);
        }
    }

    fn apply_hotkey_binding_with_probe(
        state: &mut RuntimeSettings,
        change: HotkeyBindingChange,
        probe: &HotkeyBindingProbe,
    ) -> Result<(), ()> {
        state.apply_hotkey_binding(
            change,
            (
                |shortcut| probe.register(shortcut),
                |shortcut| probe.unregister(shortcut),
            ),
            (|modifier| probe.install(modifier), || probe.uninstall()),
            (
                || probe.read_autostart(),
                |enabled| probe.change_autostart(enabled),
            ),
            || probe.persist(),
        )
    }

    fn assert_runtime_matches(actual: &RuntimeSettings, expected: &RuntimeSettings) {
        assert_eq!(actual.registered, expected.registered);
        assert_eq!(actual.installed_hook, expected.installed_hook);
    }

    #[test]
    fn double_tap_install_failure_leaves_existing_shortcut_untouched() {
        let alt_space: Shortcut = "Alt+Space".parse().unwrap();
        let mut runtime = RuntimeSettings {
            registered: vec![alt_space],
            installed_hook: None,
        };
        let probe = HotkeyBindingProbe::from_runtime(&runtime, vec![("install-Ctrl".into(), 1)]);
        let result = apply_hotkey_binding_with_probe(
            &mut runtime,
            HotkeyBindingChange {
                persisted: HotkeyKind::Chord(alt_space),
                requested: HotkeyKind::DoubleTap(DoubleTapModifier::Ctrl),
                autostart: false,
            },
            &probe,
        );
        assert!(result.is_err());
        assert_eq!(*probe.trace.borrow(), ["install-Ctrl"]);
        assert_eq!(runtime.registered, [alt_space]);
        assert_eq!(runtime.installed_hook, None);
    }

    #[test]
    fn switching_chord_to_double_tap_installs_hook_before_unregistering_shortcut() {
        let alt_space: Shortcut = "Alt+Space".parse().unwrap();
        let mut runtime = RuntimeSettings {
            registered: vec![alt_space],
            installed_hook: None,
        };
        let probe = HotkeyBindingProbe::default();
        apply_hotkey_binding_with_probe(
            &mut runtime,
            HotkeyBindingChange {
                persisted: HotkeyKind::Chord(alt_space),
                requested: HotkeyKind::DoubleTap(DoubleTapModifier::Ctrl),
                autostart: false,
            },
            &probe,
        )
        .unwrap();
        assert_eq!(
            *probe.trace.borrow(),
            [
                "install-Ctrl".into(),
                format!("unregister-{alt_space}"),
                "read-autostart".into(),
                "persist".into(),
            ]
        );
        assert!(runtime.registered.is_empty());
        assert_eq!(runtime.installed_hook, Some(DoubleTapModifier::Ctrl));
    }

    #[test]
    fn switching_double_tap_to_chord_registers_shortcut_before_uninstalling_hook() {
        let ctrl_space: Shortcut = "Ctrl+Space".parse().unwrap();
        let mut runtime = RuntimeSettings {
            registered: Vec::new(),
            installed_hook: Some(DoubleTapModifier::Alt),
        };
        let probe = HotkeyBindingProbe::default();
        apply_hotkey_binding_with_probe(
            &mut runtime,
            HotkeyBindingChange {
                persisted: HotkeyKind::DoubleTap(DoubleTapModifier::Alt),
                requested: HotkeyKind::Chord(ctrl_space),
                autostart: false,
            },
            &probe,
        )
        .unwrap();
        assert_eq!(
            *probe.trace.borrow(),
            [
                format!("register-{ctrl_space}"),
                "uninstall-hook".into(),
                "read-autostart".into(),
                "persist".into(),
            ]
        );
        assert_eq!(runtime.registered, [ctrl_space]);
        assert_eq!(runtime.installed_hook, None);
    }

    #[test]
    fn hotkey_transaction_chord_to_double_tap_failures_restore_snapshot() {
        let old: Shortcut = "Alt+Space".parse().unwrap();
        for failures in [
            vec![("install-Ctrl".into(), 1)],
            vec![(format!("unregister-{old}"), 1)],
            vec![("read-autostart".into(), 1)],
            vec![("autostart-true".into(), 1)],
            vec![("persist".into(), 1)],
        ] {
            let before = RuntimeSettings {
                registered: vec![old],
                installed_hook: None,
            };
            let mut runtime = RuntimeSettings {
                registered: before.registered.clone(),
                installed_hook: before.installed_hook,
            };
            let probe = HotkeyBindingProbe::from_runtime(&before, failures);

            assert_eq!(
                apply_hotkey_binding_with_probe(
                    &mut runtime,
                    HotkeyBindingChange {
                        persisted: HotkeyKind::Chord(old),
                        requested: HotkeyKind::DoubleTap(DoubleTapModifier::Ctrl),
                        autostart: true,
                    },
                    &probe,
                ),
                Err(())
            );
            assert_runtime_matches(&runtime, &before);
            probe.assert_actual_matches(&before);
        }
    }

    #[test]
    fn hotkey_transaction_double_tap_to_chord_failures_restore_snapshot() {
        let requested: Shortcut = "Ctrl+Space".parse().unwrap();
        for failures in [
            vec![(format!("register-{requested}"), 1)],
            vec![("uninstall-hook".into(), 1)],
            vec![("read-autostart".into(), 1)],
            vec![("autostart-true".into(), 1)],
            vec![("persist".into(), 1)],
        ] {
            let before = RuntimeSettings {
                registered: Vec::new(),
                installed_hook: Some(DoubleTapModifier::Alt),
            };
            let mut runtime = RuntimeSettings {
                registered: before.registered.clone(),
                installed_hook: before.installed_hook,
            };
            let probe = HotkeyBindingProbe::from_runtime(&before, failures);

            assert_eq!(
                apply_hotkey_binding_with_probe(
                    &mut runtime,
                    HotkeyBindingChange {
                        persisted: HotkeyKind::DoubleTap(DoubleTapModifier::Alt),
                        requested: HotkeyKind::Chord(requested),
                        autostart: true,
                    },
                    &probe,
                ),
                Err(())
            );
            assert_runtime_matches(&runtime, &before);
            probe.assert_actual_matches(&before);
        }
    }

    #[test]
    fn hotkey_transaction_modifier_failures_restore_snapshot() {
        for failures in [
            vec![("uninstall-hook".into(), 1)],
            vec![("install-Alt".into(), 1)],
            vec![("read-autostart".into(), 1)],
            vec![("autostart-true".into(), 1)],
            vec![("persist".into(), 1)],
        ] {
            let before = RuntimeSettings {
                registered: Vec::new(),
                installed_hook: Some(DoubleTapModifier::Ctrl),
            };
            let mut runtime = RuntimeSettings {
                registered: before.registered.clone(),
                installed_hook: before.installed_hook,
            };
            let probe = HotkeyBindingProbe::from_runtime(&before, failures);

            assert_eq!(
                apply_hotkey_binding_with_probe(
                    &mut runtime,
                    HotkeyBindingChange {
                        persisted: HotkeyKind::DoubleTap(DoubleTapModifier::Ctrl),
                        requested: HotkeyKind::DoubleTap(DoubleTapModifier::Alt),
                        autostart: true,
                    },
                    &probe,
                ),
                Err(())
            );
            assert_runtime_matches(&runtime, &before);
            probe.assert_actual_matches(&before);
        }
    }

    #[test]
    fn hotkey_transaction_rollback_failure_keeps_observed_actual_state() {
        let before = RuntimeSettings {
            registered: Vec::new(),
            installed_hook: Some(DoubleTapModifier::Ctrl),
        };
        let mut runtime = RuntimeSettings {
            registered: before.registered.clone(),
            installed_hook: before.installed_hook,
        };
        let probe = HotkeyBindingProbe::from_runtime(
            &before,
            vec![("install-Alt".into(), 1), ("install-Ctrl".into(), 1)],
        );

        assert_eq!(
            apply_hotkey_binding_with_probe(
                &mut runtime,
                HotkeyBindingChange {
                    persisted: HotkeyKind::DoubleTap(DoubleTapModifier::Ctrl),
                    requested: HotkeyKind::DoubleTap(DoubleTapModifier::Alt),
                    autostart: false,
                },
                &probe,
            ),
            Err(())
        );
        probe.assert_actual_matches(&runtime);
        assert!(!probe
            .trace
            .borrow()
            .iter()
            .all(|operation| operation != "install-Ctrl"));
        assert_ne!(runtime.installed_hook, before.installed_hook);
    }

    #[test]
    fn hotkey_transaction_removes_new_chord_when_old_hook_restore_fails() {
        let requested: Shortcut = "Ctrl+Space".parse().unwrap();
        let before = RuntimeSettings {
            registered: Vec::new(),
            installed_hook: Some(DoubleTapModifier::Ctrl),
        };
        let mut runtime = before.clone();
        let probe = HotkeyBindingProbe::from_runtime(
            &before,
            vec![("persist".into(), 1), ("install-Ctrl".into(), 1)],
        );

        assert_eq!(
            apply_hotkey_binding_with_probe(
                &mut runtime,
                HotkeyBindingChange {
                    persisted: HotkeyKind::DoubleTap(DoubleTapModifier::Ctrl),
                    requested: HotkeyKind::Chord(requested),
                    autostart: false,
                },
                &probe,
            ),
            Err(())
        );
        assert_eq!(runtime.registered, []);
        assert_eq!(runtime.installed_hook, None);
        assert_eq!(
            probe.trace.borrow().last().unwrap(),
            &format!("unregister-{requested}")
        );
        probe.assert_actual_matches(&runtime);
    }

    #[test]
    fn hotkey_transaction_removes_new_chord_when_old_chord_restore_fails() {
        let old: Shortcut = "Alt+Space".parse().unwrap();
        let requested: Shortcut = "Ctrl+Space".parse().unwrap();
        let before = RuntimeSettings {
            registered: vec![old],
            installed_hook: None,
        };
        let mut runtime = before.clone();
        let probe = HotkeyBindingProbe::from_runtime(
            &before,
            vec![("persist".into(), 1), (format!("register-{old}"), 1)],
        );

        assert_eq!(
            apply_hotkey_binding_with_probe(
                &mut runtime,
                HotkeyBindingChange {
                    persisted: HotkeyKind::Chord(old),
                    requested: HotkeyKind::Chord(requested),
                    autostart: false,
                },
                &probe,
            ),
            Err(())
        );
        assert_eq!(runtime.registered, []);
        assert_eq!(runtime.installed_hook, None);
        assert_eq!(
            probe.trace.borrow().last().unwrap(),
            &format!("unregister-{requested}")
        );
        probe.assert_actual_matches(&runtime);
    }

    #[test]
    fn hotkey_transaction_persistence_is_last() {
        let old: Shortcut = "Alt+Space".parse().unwrap();
        let requested: Shortcut = "Ctrl+Space".parse().unwrap();
        let before = RuntimeSettings {
            registered: vec![old],
            installed_hook: None,
        };
        let mut runtime = RuntimeSettings {
            registered: before.registered.clone(),
            installed_hook: before.installed_hook,
        };
        let probe = HotkeyBindingProbe::from_runtime(&before, Vec::new());

        apply_hotkey_binding_with_probe(
            &mut runtime,
            HotkeyBindingChange {
                persisted: HotkeyKind::Chord(old),
                requested: HotkeyKind::Chord(requested),
                autostart: false,
            },
            &probe,
        )
        .unwrap();
        assert_eq!(
            probe.trace.borrow().last().map(String::as_str),
            Some("persist")
        );
    }

    #[test]
    fn reconcile_double_alt_installs_hook_without_shortcut_register() {
        let coordinator = coordinator_for_test();
        let register_calls = Cell::new(0);
        let install_calls = Cell::new(0);
        coordinator
            .reconcile_runtime_settings_with(
                DOUBLE_ALT,
                false,
                (
                    HotkeyKind::parse,
                    |_| {
                        register_calls.set(register_calls.get() + 1);
                        Ok(())
                    },
                    |modifier| {
                        assert_eq!(modifier, DoubleTapModifier::Alt);
                        install_calls.set(install_calls.get() + 1);
                        Ok(())
                    },
                ),
                (|| Ok(false), |_| Ok(())),
            )
            .unwrap();
        assert_eq!(register_calls.get(), 0);
        assert_eq!(install_calls.get(), 1);
        assert_eq!(
            coordinator
                .runtime_settings
                .lock()
                .expect("runtime settings lock poisoned")
                .installed_hook,
            Some(DoubleTapModifier::Alt)
        );
        assert!(HotkeyKind::parse(DOUBLE_CTRL).is_ok());
    }

    #[test]
    fn file_index_phase_mirror() {
        let coordinator = coordinator_for_test();
        assert_eq!(coordinator.file_index_phase(), FileIndexPhase::Running);
        assert_eq!(coordinator.file_index_attempt_epoch(), 0);

        let deadline = Instant::now() + Duration::from_secs(1);
        assert_ne!(
            coordinator.begin_tray_clean(deadline),
            CleanDecision::ObserveOnly
        );
        assert_eq!(coordinator.file_index_phase(), FileIndexPhase::Cleaning);
        assert_eq!(coordinator.file_index_attempt_epoch(), 1);

        assert_eq!(
            coordinator.complete_clean(CleanResult::Failed),
            CleanDecision::ReturnRunning
        );
        assert_eq!(coordinator.file_index_phase(), FileIndexPhase::Running);
        assert_eq!(coordinator.file_index_attempt_epoch(), 1);

        let _ = coordinator.begin_system_end_nonblocking(Instant::now());
        assert_eq!(coordinator.file_index_phase(), FileIndexPhase::Terminal);
        assert_eq!(coordinator.file_index_attempt_epoch(), 1);
    }
}
