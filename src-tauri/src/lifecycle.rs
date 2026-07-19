use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
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

#[derive(Debug)]
struct RuntimeSettings<T = Shortcut> {
    registered: Vec<T>,
}

impl<T> Default for RuntimeSettings<T> {
    fn default() -> Self {
        Self {
            registered: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RuntimeSettingsChange<T> {
    persisted: T,
    requested: T,
    autostart: bool,
}

impl<T> RuntimeSettings<T>
where
    T: Copy + Eq,
{
    fn apply_transaction<R, U, A, C, P>(
        &mut self,
        change: RuntimeSettingsChange<T>,
        mut register: R,
        mut unregister: U,
        read_autostart: A,
        mut change_autostart: C,
        persist: P,
    ) -> Result<(), ()>
    where
        R: FnMut(T) -> Result<(), ()>,
        U: FnMut(T) -> Result<(), ()>,
        A: FnOnce() -> Result<bool, ()>,
        C: FnMut(bool) -> Result<(), ()>,
        P: FnOnce() -> Result<(), ()>,
    {
        let mut index = 0;
        while index < self.registered.len() {
            let shortcut = self.registered[index];
            if shortcut == change.persisted {
                index += 1;
            } else {
                unregister(shortcut)?;
                self.registered.remove(index);
            }
        }

        let owned_new = if self.registered.contains(&change.requested) {
            false
        } else {
            register(change.requested)?;
            self.registered.push(change.requested);
            true
        };

        let previous_autostart = match read_autostart() {
            Ok(enabled) => enabled,
            Err(()) => {
                self.rollback_registration(change.requested, owned_new, &mut unregister);
                return Err(());
            }
        };
        let changed_autostart = previous_autostart != change.autostart;
        if changed_autostart && change_autostart(change.autostart).is_err() {
            self.rollback_registration(change.requested, owned_new, &mut unregister);
            return Err(());
        }

        if persist().is_err() {
            if changed_autostart {
                let _ = change_autostart(previous_autostart);
            }
            self.rollback_registration(change.requested, owned_new, &mut unregister);
            return Err(());
        }

        if change.requested != change.persisted && self.registered.contains(&change.persisted) {
            unregister(change.persisted)?;
            self.registered
                .retain(|shortcut| *shortcut != change.persisted);
        }
        Ok(())
    }

    fn rollback_registration<U>(&mut self, shortcut: T, owned: bool, unregister: &mut U)
    where
        U: FnMut(T) -> Result<(), ()>,
    {
        if owned && unregister(shortcut).is_ok() {
            self.registered.retain(|registered| *registered != shortcut);
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
    readiness: Mutex<Readiness>,
    modal: Mutex<ModalState>,
    exit_gate: Mutex<ExitGate>,
    critical_changed: Condvar,
    pending_notice: Mutex<Option<LifecycleNotice>>,
    runtime_settings: Mutex<RuntimeSettings>,
}

impl Default for LifecycleCoordinator {
    fn default() -> Self {
        Self {
            next_invocation: AtomicU64::new(0),
            readiness: Mutex::new(Readiness::default()),
            modal: Mutex::new(ModalState::Normal),
            exit_gate: Mutex::new(ExitGate::default()),
            critical_changed: Condvar::new(),
            pending_notice: Mutex::new(None),
            runtime_settings: Mutex::new(RuntimeSettings::default()),
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
    pub(crate) fn save_settings_transaction(
        &self,
        app: &AppHandle,
        settings: &SettingsStore,
        cache: &crate::apps::AppCache,
        shortcut: Shortcut,
        update: SettingsUpdate,
    ) -> Result<(), ()> {
        let mut runtime = self
            .runtime_settings
            .lock()
            .expect("runtime settings lock poisoned");
        let persisted = settings.snapshot();
        let persisted_shortcut = persisted.hotkey.parse::<Shortcut>().map_err(|_| ())?;
        let global_shortcut = app.global_shortcut();
        let autostart = app.autolaunch();
        runtime.apply_transaction(
            RuntimeSettingsChange {
                persisted: persisted_shortcut,
                requested: shortcut,
                autostart: update.autostart,
            },
            |shortcut| global_shortcut.register(shortcut).map_err(|_| ()),
            |shortcut| global_shortcut.unregister(shortcut).map_err(|_| ()),
            || autostart.is_enabled().map_err(|_| ()),
            |enabled| {
                if enabled {
                    autostart.enable()
                } else {
                    autostart.disable()
                }
                .map_err(|_| ())
            },
            || settings.update_user_settings(update, cache).map_err(|_| ()),
        )
    }

    pub(crate) fn reconcile_runtime_settings(
        &self,
        app: &AppHandle,
        settings: &Settings,
    ) -> Result<(), ()> {
        let global_shortcut = app.global_shortcut();
        let autostart = app.autolaunch();
        self.reconcile_runtime_settings_with(
            &settings.hotkey,
            settings.autostart,
            |value| value.parse::<Shortcut>().map_err(|_| ()),
            |shortcut| global_shortcut.register(shortcut).map_err(|_| ()),
            || autostart.is_enabled().map_err(|_| ()),
            |enabled| {
                if enabled {
                    autostart.enable()
                } else {
                    autostart.disable()
                }
                .map_err(|_| ())
            },
        )
    }

    fn reconcile_runtime_settings_with<P, R, A, C>(
        &self,
        hotkey: &str,
        expected_autostart: bool,
        parse: P,
        register: R,
        read_autostart: A,
        change_autostart: C,
    ) -> Result<(), ()>
    where
        P: FnOnce(&str) -> Result<Shortcut, ()>,
        R: FnOnce(Shortcut) -> Result<(), ()>,
        A: FnOnce() -> Result<bool, ()>,
        C: FnOnce(bool) -> Result<(), ()>,
    {
        let result = (|| {
            let shortcut = parse(hotkey)?;
            register(shortcut)?;
            self.runtime_settings
                .lock()
                .expect("runtime settings lock poisoned")
                .registered
                .push(shortcut);
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

    fn begin_tray_clean(&self, deadline: Instant) -> CleanDecision {
        let mut gate = self.exit_gate.lock().expect("exit gate lock poisoned");
        if gate.state != ExitState::Running || !matches!(gate.clean_attempt, CleanAttempt::Idle) {
            return CleanDecision::ObserveOnly;
        }

        gate.state = ExitState::Cleaning;
        Self::start_waiting(&mut gate, CleanOwner::Tray, deadline)
    }

    fn begin_system_end(&self, deadline: Instant) -> CleanDecision {
        let mut gate = self.exit_gate.lock().expect("exit gate lock poisoned");
        gate.state = ExitState::SystemEnding;

        match gate.clean_attempt {
            CleanAttempt::Idle => Self::start_waiting(&mut gate, CleanOwner::System, deadline),
            CleanAttempt::Waiting { deadline, .. } | CleanAttempt::Calling { deadline, .. } => {
                CleanDecision::Wait { deadline }
            }
            CleanAttempt::Finished(_) => CleanDecision::ObserveOnly,
        }
    }

    fn advance_clean(&self, now: Instant) -> CleanDecision {
        let mut gate = self.exit_gate.lock().expect("exit gate lock poisoned");
        match gate.clean_attempt {
            CleanAttempt::Waiting { owner, deadline } if now >= deadline => {
                let decision = if owner == CleanOwner::Tray && gate.state == ExitState::Cleaning {
                    gate.state = ExitState::Running;
                    gate.clean_attempt = CleanAttempt::Idle;
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
                    CleanDecision::Exit
                }
                CleanResult::Failed | CleanResult::TimedOut => {
                    gate.state = ExitState::Running;
                    gate.clean_attempt = CleanAttempt::Idle;
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

    fn run_system_end_with<W, M>(&self, deadline: Instant, wait: W, marker: M) -> CleanDecision
    where
        W: FnMut(Instant) -> Instant,
        M: FnOnce() -> CleanResult,
    {
        let decision = self.begin_system_end(deadline);
        self.run_clean_attempt_with(decision, wait, marker)
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
        let decision = self.begin_tray_clean(Instant::now() + Duration::from_secs(1));
        if decision == CleanDecision::ObserveOnly {
            return;
        }

        let coordinator = Arc::clone(self);
        let app = app.clone();
        drop(tauri::async_runtime::spawn_blocking(move || {
            let marker_app = app.clone();
            let exit_dispatcher = app.clone();
            let exit_app = app.clone();
            let show_app = app.clone();
            let show_coordinator = Arc::clone(&coordinator);
            coordinator.run_tray_quit_with(
                decision,
                |deadline| coordinator.wait_for_clean_change(deadline),
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
                move || {
                    let app = exit_app.clone();
                    let _ = exit_dispatcher.run_on_main_thread(move || app.exit(0));
                },
                move |target| {
                    let _ = show_coordinator.request_show(&show_app, target);
                },
            );
        }));
    }

    fn run_system_end(&self, app: &AppHandle) {
        let marker_app = app.clone();
        self.run_system_end_with(
            Instant::now() + Duration::from_secs(1),
            |deadline| self.wait_for_clean_change(deadline),
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
    use crate::{commands::save_settings_worker_with, result_registry::ResultRegistry};
    use tauri_plugin_global_shortcut::Shortcut;

    const _: fn(&Arc<LifecycleCoordinator>, &AppHandle, ShowTarget) -> Result<(), LifecycleError> =
        LifecycleCoordinator::request_show;
    const _: fn(&Arc<LifecycleCoordinator>, &AppHandle) -> Result<(), LifecycleError> =
        LifecycleCoordinator::mark_setup_ready;
    const _: fn(&Arc<LifecycleCoordinator>, &AppHandle) = LifecycleCoordinator::request_tray_quit;
    const _: fn(&LifecycleCoordinator, &AppHandle, &Settings) -> Result<(), ()> =
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
        assert!(registry.begin_query("old", 1).is_some());
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
        assert!(registry.begin_query("old", 2).is_none());
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
    fn clean_attempt_system_shares_waiting_and_calling_tray_attempt() {
        let coordinator = coordinator_for_test();
        let reservation = coordinator.reserve_critical().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let later_deadline = deadline + Duration::from_secs(5);
        let mut trace = AttemptTrace::default();

        let (tray_trace, tray) =
            trace.begin(&coordinator, || coordinator.begin_tray_clean(deadline));
        assert_eq!(tray, CleanDecision::Wait { deadline });
        let system = coordinator.begin_system_end(later_deadline);
        assert_eq!(system, CleanDecision::Wait { deadline });
        assert_eq!(trace.observe(), tray_trace);
        assert_eq!(exit_snapshot(&coordinator).0, ExitState::SystemEnding);

        drop(reservation);
        assert_eq!(
            coordinator.advance_clean(Instant::now()),
            CleanDecision::CallMarker
        );
        assert!(matches!(
            exit_snapshot(&coordinator).2,
            CleanAttempt::Calling {
                owner: CleanOwner::Tray,
                deadline: stored
            } if stored == deadline
        ));
        assert_eq!(
            coordinator.begin_system_end(later_deadline),
            CleanDecision::Wait { deadline }
        );
        assert_eq!(trace.observe(), tray_trace);

        let marker_calls = Cell::new(0);
        let completion = complete_requested_clean(&coordinator, CleanDecision::CallMarker, || {
            marker_calls.set(marker_calls.get() + 1);
            CleanResult::Succeeded
        });
        assert_eq!(completion, CleanDecision::ObserveOnly);
        assert_eq!(marker_calls.get(), 1);
        assert_eq!(exit_snapshot(&coordinator).0, ExitState::SystemEnding);
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
                coordinator.begin_system_end(deadline + Duration::from_secs(1)),
                CleanDecision::Wait { deadline }
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
            succeeded.begin_system_end(success_deadline + Duration::from_secs(1)),
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
            failed.begin_system_end(deadline + Duration::from_secs(1))
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
            timed_out.begin_system_end(timeout_deadline + Duration::from_secs(1))
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
            shared.begin_system_end(shared_deadline + Duration::from_secs(1)),
            CleanDecision::Wait {
                deadline: shared_deadline
            }
        );
        assert_eq!(
            shared.advance_clean(shared_deadline),
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
            released_shared.begin_system_end(released_shared_deadline + Duration::from_secs(1)),
            CleanDecision::Wait {
                deadline: released_shared_deadline
            }
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
            system.begin_system_end(system_deadline),
            CleanDecision::Wait {
                deadline: system_deadline
            }
        );
        assert_eq!(
            system.advance_clean(system_deadline),
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

    struct RuntimeProbe {
        trace: RefCell<Vec<String>>,
        autostart: Cell<Result<bool, ()>>,
        register_failure: Cell<Option<&'static str>>,
        unregister_failures: RefCell<Vec<&'static str>>,
        autostart_failure_on_call: Cell<Option<usize>>,
        autostart_calls: Cell<usize>,
        persist_failure: Cell<bool>,
        persist_calls: Cell<usize>,
    }

    impl Default for RuntimeProbe {
        fn default() -> Self {
            Self {
                trace: RefCell::new(Vec::new()),
                autostart: Cell::new(Ok(false)),
                register_failure: Cell::new(None),
                unregister_failures: RefCell::new(Vec::new()),
                autostart_failure_on_call: Cell::new(None),
                autostart_calls: Cell::new(0),
                persist_failure: Cell::new(false),
                persist_calls: Cell::new(0),
            }
        }
    }

    impl RuntimeProbe {
        fn register(&self, shortcut: &'static str) -> Result<(), ()> {
            self.trace.borrow_mut().push(format!("register-{shortcut}"));
            (self.register_failure.get() != Some(shortcut))
                .then_some(())
                .ok_or(())
        }

        fn unregister(&self, shortcut: &'static str) -> Result<(), ()> {
            self.trace
                .borrow_mut()
                .push(format!("unregister-{shortcut}"));
            (!self.unregister_failures.borrow().contains(&shortcut))
                .then_some(())
                .ok_or(())
        }

        fn read_autostart(&self) -> Result<bool, ()> {
            self.trace.borrow_mut().push("read-autostart".into());
            self.autostart.get()
        }

        fn change_autostart(&self, enabled: bool) -> Result<(), ()> {
            let call = self.autostart_calls.get() + 1;
            self.autostart_calls.set(call);
            self.trace.borrow_mut().push(format!("autostart-{enabled}"));
            if self.autostart_failure_on_call.get() == Some(call) {
                return Err(());
            }
            self.autostart.set(Ok(enabled));
            Ok(())
        }

        fn persist(&self) -> Result<(), ()> {
            self.persist_calls.set(self.persist_calls.get() + 1);
            self.trace.borrow_mut().push("persist".into());
            (!self.persist_failure.get()).then_some(()).ok_or(())
        }
    }

    fn apply_runtime_change(
        state: &mut RuntimeSettings<&'static str>,
        change: RuntimeSettingsChange<&'static str>,
        probe: &RuntimeProbe,
    ) -> Result<(), ()> {
        state.apply_transaction(
            change,
            |shortcut| probe.register(shortcut),
            |shortcut| probe.unregister(shortcut),
            || probe.read_autostart(),
            |enabled| probe.change_autostart(enabled),
            || probe.persist(),
        )
    }

    #[test]
    fn runtime_settings_enforces_two_registration_ceiling() {
        let mut state = RuntimeSettings {
            registered: vec!["A"],
        };
        let first = RuntimeProbe {
            autostart: Cell::new(Ok(false)),
            unregister_failures: RefCell::new(vec!["A"]),
            ..RuntimeProbe::default()
        };

        assert_eq!(
            apply_runtime_change(
                &mut state,
                RuntimeSettingsChange {
                    persisted: "A",
                    requested: "B",
                    autostart: false,
                },
                &first,
            ),
            Err(())
        );
        assert_eq!(state.registered, ["A", "B"]);
        assert_eq!(first.persist_calls.get(), 1);

        let second = RuntimeProbe {
            autostart: Cell::new(Ok(false)),
            unregister_failures: RefCell::new(vec!["A"]),
            ..RuntimeProbe::default()
        };
        assert_eq!(
            apply_runtime_change(
                &mut state,
                RuntimeSettingsChange {
                    persisted: "B",
                    requested: "C",
                    autostart: false,
                },
                &second,
            ),
            Err(())
        );
        assert_eq!(state.registered, ["A", "B"]);
        assert_eq!(second.persist_calls.get(), 0);
        assert_eq!(
            *second.trace.borrow(),
            ["unregister-A"],
            "stale cleanup must precede every new side effect"
        );
    }

    #[test]
    fn runtime_settings_cleans_stale_before_registering_next_shortcut() {
        let mut state = RuntimeSettings {
            registered: vec!["A", "B"],
        };
        let probe = RuntimeProbe {
            autostart: Cell::new(Ok(false)),
            ..RuntimeProbe::default()
        };

        assert_eq!(
            apply_runtime_change(
                &mut state,
                RuntimeSettingsChange {
                    persisted: "B",
                    requested: "C",
                    autostart: false,
                },
                &probe,
            ),
            Ok(())
        );
        assert_eq!(state.registered, ["C"]);
        assert_eq!(
            *probe.trace.borrow(),
            [
                "unregister-A",
                "register-C",
                "read-autostart",
                "persist",
                "unregister-B",
            ]
        );

        let conflict = RuntimeProbe {
            autostart: Cell::new(Ok(false)),
            register_failure: Cell::new(Some("B")),
            ..RuntimeProbe::default()
        };
        let mut state = RuntimeSettings {
            registered: vec!["A"],
        };
        assert_eq!(
            apply_runtime_change(
                &mut state,
                RuntimeSettingsChange {
                    persisted: "A",
                    requested: "B",
                    autostart: false,
                },
                &conflict,
            ),
            Err(())
        );
        assert_eq!(state.registered, ["A"]);
        assert_eq!(*conflict.trace.borrow(), ["register-B"]);

        let recovery = RuntimeProbe {
            autostart: Cell::new(Ok(false)),
            unregister_failures: RefCell::new(vec!["A"]),
            ..RuntimeProbe::default()
        };
        let mut state = RuntimeSettings::default();
        assert_eq!(
            apply_runtime_change(
                &mut state,
                RuntimeSettingsChange {
                    persisted: "A",
                    requested: "B",
                    autostart: false,
                },
                &recovery,
            ),
            Ok(())
        );
        assert_eq!(state.registered, ["B"]);
        assert_eq!(
            *recovery.trace.borrow(),
            ["register-B", "read-autostart", "persist"]
        );
    }

    #[test]
    fn runtime_settings_orders_autostart_and_persistence_without_redundant_changes() {
        let mut unchanged = RuntimeSettings {
            registered: vec!["A"],
        };
        let unchanged_probe = RuntimeProbe {
            autostart: Cell::new(Ok(false)),
            ..RuntimeProbe::default()
        };
        assert_eq!(
            apply_runtime_change(
                &mut unchanged,
                RuntimeSettingsChange {
                    persisted: "A",
                    requested: "A",
                    autostart: false,
                },
                &unchanged_probe,
            ),
            Ok(())
        );
        assert_eq!(
            *unchanged_probe.trace.borrow(),
            ["read-autostart", "persist"]
        );

        let mut changed = RuntimeSettings {
            registered: vec!["A"],
        };
        let changed_probe = RuntimeProbe {
            autostart: Cell::new(Ok(false)),
            ..RuntimeProbe::default()
        };
        assert_eq!(
            apply_runtime_change(
                &mut changed,
                RuntimeSettingsChange {
                    persisted: "A",
                    requested: "B",
                    autostart: true,
                },
                &changed_probe,
            ),
            Ok(())
        );
        assert_eq!(changed.registered, ["B"]);
        assert_eq!(
            *changed_probe.trace.borrow(),
            [
                "register-B",
                "read-autostart",
                "autostart-true",
                "persist",
                "unregister-A",
            ]
        );
    }

    #[test]
    fn runtime_settings_rolls_back_only_owned_changes_after_persist_failure() {
        let mut state = RuntimeSettings {
            registered: vec!["A"],
        };
        let probe = RuntimeProbe {
            autostart: Cell::new(Ok(false)),
            persist_failure: Cell::new(true),
            ..RuntimeProbe::default()
        };
        assert_eq!(
            apply_runtime_change(
                &mut state,
                RuntimeSettingsChange {
                    persisted: "A",
                    requested: "B",
                    autostart: true,
                },
                &probe,
            ),
            Err(())
        );
        assert_eq!(state.registered, ["A"]);
        assert_eq!(probe.autostart.get(), Ok(false));
        assert_eq!(
            *probe.trace.borrow(),
            [
                "register-B",
                "read-autostart",
                "autostart-true",
                "persist",
                "autostart-false",
                "unregister-B",
            ]
        );

        for (rollback_autostart_fails, rollback_unregister_fails) in
            [(true, false), (false, true), (true, true)]
        {
            let mut state = RuntimeSettings {
                registered: vec!["A"],
            };
            let probe = RuntimeProbe {
                autostart: Cell::new(Ok(false)),
                unregister_failures: RefCell::new(
                    rollback_unregister_fails
                        .then_some("B")
                        .into_iter()
                        .collect(),
                ),
                autostart_failure_on_call: Cell::new(rollback_autostart_fails.then_some(2)),
                persist_failure: Cell::new(true),
                ..RuntimeProbe::default()
            };
            assert_eq!(
                apply_runtime_change(
                    &mut state,
                    RuntimeSettingsChange {
                        persisted: "A",
                        requested: "B",
                        autostart: true,
                    },
                    &probe,
                ),
                Err(())
            );
            assert!(state.registered.len() <= 2);
            assert!(state.registered.contains(&"A"));
            assert_eq!(state.registered.contains(&"B"), rollback_unregister_fails);
        }
    }

    #[test]
    fn runtime_settings_autostart_failures_skip_persist_and_remove_owned_registration() {
        for (read_result, change_failure) in [(Err(()), None), (Ok(false), Some(1))] {
            let mut state = RuntimeSettings {
                registered: vec!["A"],
            };
            let probe = RuntimeProbe {
                autostart: Cell::new(read_result),
                autostart_failure_on_call: Cell::new(change_failure),
                ..RuntimeProbe::default()
            };
            assert_eq!(
                apply_runtime_change(
                    &mut state,
                    RuntimeSettingsChange {
                        persisted: "A",
                        requested: "B",
                        autostart: true,
                    },
                    &probe,
                ),
                Err(())
            );
            assert_eq!(state.registered, ["A"]);
            assert_eq!(probe.persist_calls.get(), 0);
            assert_eq!(probe.trace.borrow().last().unwrap(), "unregister-B");
        }
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
                |value| value.parse::<Shortcut>().map_err(|_| ()),
                |registered| {
                    assert_eq!(registered, shortcut);
                    trace.borrow_mut().push("register");
                    Ok(())
                },
                || {
                    trace.borrow_mut().push("read-autostart");
                    Ok(false)
                },
                |enabled| {
                    assert!(enabled);
                    trace.borrow_mut().push("autostart-true");
                    Ok(())
                },
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
                |value| value.parse::<Shortcut>().map_err(|_| ()),
                |registered| {
                    assert_eq!(registered, shortcut);
                    matching_registers.set(matching_registers.get() + 1);
                    Ok(())
                },
                || {
                    matching_reads.set(matching_reads.get() + 1);
                    Ok(false)
                },
                |_| {
                    matching_changes.set(matching_changes.get() + 1);
                    Ok(())
                },
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
                |value| value.parse::<Shortcut>().map_err(|_| ()),
                |_| if register_fails { Err(()) } else { Ok(()) },
                || panic!("failed shortcut setup must skip autostart read"),
                |_| panic!("failed shortcut setup must skip autostart change"),
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
                |value| value.parse::<Shortcut>().map_err(|_| ()),
                |registered| {
                    assert_eq!(registered, shortcut);
                    read_failure_registers.set(read_failure_registers.get() + 1);
                    Ok(())
                },
                || Err(()),
                |_| {
                    read_failure_changes.set(read_failure_changes.get() + 1);
                    Ok(())
                },
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
                |value| value.parse::<Shortcut>().map_err(|_| ()),
                |registered| {
                    assert_eq!(registered, shortcut);
                    change_failure_registers.set(change_failure_registers.get() + 1);
                    Ok(())
                },
                || Ok(false),
                |_| {
                    change_calls.set(change_calls.get() + 1);
                    Err(())
                },
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
                coordinator.begin_system_end(deadline + Duration::from_secs(1)),
                CleanDecision::Wait { deadline }
            );
            assert_eq!(
                coordinator.advance_clean(deadline),
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
            "autostart",
            "persist",
            "autostart-rollback",
            "rollback-unregister",
        ] {
            let coordinator = coordinator_for_test();
            let reserve_coordinator = Arc::clone(&coordinator);
            let worker_coordinator = Arc::clone(&coordinator);
            let mut state = RuntimeSettings {
                registered: if phase == "stale-cleanup" {
                    vec!["stale", "A"]
                } else {
                    vec!["A"]
                },
            };
            let autostart_calls = Cell::new(0);
            let persist_fails = matches!(phase, "autostart-rollback" | "rollback-unregister");
            let result = tauri::async_runtime::block_on(save_settings_worker_with(
                move || reserve_coordinator.reserve_critical().map_err(|_| ()),
                move |reservation| {
                    let _reservation = reservation;
                    state.apply_transaction(
                        RuntimeSettingsChange {
                            persisted: "A",
                            requested: "B",
                            autostart: true,
                        },
                        |_| {
                            if phase == "register" {
                                assert_clean_blocked(&worker_coordinator);
                            }
                            Ok(())
                        },
                        |shortcut| {
                            if (phase == "stale-cleanup" && shortcut == "stale")
                                || (phase == "rollback-unregister" && shortcut == "B")
                            {
                                assert_clean_blocked(&worker_coordinator);
                            }
                            Ok(())
                        },
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
            tray_action(TRAY_OPEN_SETTINGS),
            Some(TrayAction::Show(ShowTarget::Settings))
        );
        assert_eq!(tray_action(TRAY_QUIT), Some(TrayAction::Quit));
        for rejected in [
            "open-settings",
            "quit",
            "uipilot.tray.open",
            "UIPILOT.TRAY.QUIT",
            "uipilot.tray.quit ",
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
                || exit_calls.set(exit_calls.get() + 1),
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
            shared.begin_system_end(deadline),
            CleanDecision::Wait { deadline }
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
                        coordinator.run_system_end_with(
                            deadline,
                            |_| panic!("fresh system cleanup must call marker immediately"),
                            || {
                                marker_calls.set(marker_calls.get() + 1);
                                CleanResult::Succeeded
                            },
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
            coordinator.run_system_end_with(
                deadline,
                |_| panic!("finished success must not wait"),
                || {
                    marker_calls.set(marker_calls.get() + 1);
                    CleanResult::Succeeded
                },
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
                coordinator.run_system_end_with(
                    deadline,
                    |_| panic!("fresh system attempt must call marker immediately"),
                    || {
                        system_marker_calls.set(system_marker_calls.get() + 1);
                        CleanResult::Succeeded
                    },
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
}
