//! Host scheduling/data/publication wiring, not a production execution engine.
//! Only tests can provide a transport. The normal constructor is closed and
//! never discovers inventory, launches a process, or evaluates plugin source.
pub(crate) use super::request_contract::PluginInvocationTheme as InvocationTheme;
use super::{
    lifecycle_macos::LifecycleError,
    main_result_macos::{MainResultOwner, MainResultPublisher},
    management_macos::{ManagementEpoch, ManagementState, RuntimeInvalidation},
    manifest::PublicActivationMode,
    package_macos::VerifiedPackage,
    request_contract::{PluginInvocation, PluginInvocationEnvironment, PluginInvocationPlatform},
    runtime_data_macos::RuntimeDataBinding,
    runtime_result_macos::MainCompletionError,
};
use crate::{
    clipboard_history::{HostCopyError, HostCopyOutcome},
    model::{
        search_response, LauncherResultActivation, MainResultCommandContext, ResultItem,
        SearchResponse,
    },
    result_registry::{
        MainActionCompletionOwner, MainActionKind, QueryDomain, ResultAction, ResultRegistry,
    },
};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
enum CompletionPhase {
    Preview,
    Commit,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CompletionOrigin {
    phase: CompletionPhase,
    plugin_id: String,
    command_id: Option<String>,
}

#[derive(Debug)]
pub(crate) enum Route {
    NotHandled,
    Published(SearchResponse),
    Stale,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error {
    Unavailable,
    Storage,
    InvalidResult,
    UnsupportedAction,
}
impl Error {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Unavailable => "pluginRuntimeUnavailable",
            Self::Storage => "pluginRuntimeStorage",
            Self::InvalidResult => "pluginRuntimeInvalidResult",
            Self::UnsupportedAction => "pluginRuntimeUnsupportedAction",
        }
    }
}

// No production constructor, public pipe configuration, OS handle, source path
// or arbitrary callback from IPC. A future trusted adapter must verify normal
// transport termination before constructing this result; raw JSON is not proof.
struct TerminalResult {
    raw: Vec<u8>,
}
struct RuntimeInput {
    invocation: PluginInvocation,
    package: VerifiedPackage,
    binding: RuntimeDataBinding,
    deadline: Instant,
}
trait RuntimeTransport: Send + Sync {
    fn run(&self, input: RuntimeInput, cancel: Cancellation) -> Result<TerminalResult, Error>;
}
#[derive(Clone)]
struct Cancellation(Arc<Control>);
impl Cancellation {
    fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }
    #[cfg(test)]
    fn wait(&self, timeout: Duration) -> bool {
        let state = self.0.state.lock().unwrap();
        let _ = self
            .0
            .changed
            .wait_timeout_while(state, timeout, |_| !self.is_cancelled())
            .unwrap();
        self.is_cancelled()
    }
}
#[derive(Default)]
struct RunLife {
    started: bool,
    finished: bool,
}
#[derive(Default)]
struct Control {
    cancelled: AtomicBool,
    state: Mutex<RunLife>,
    changed: Condvar,
}
struct ActiveRun {
    identity: Arc<()>,
    management_epoch: ManagementEpoch,
    binding: RuntimeDataBinding,
    publisher: Arc<MainResultPublisher>,
    publication_owner: Arc<MainResultOwner>,
    control: Arc<Control>,
}
impl ActiveRun {
    fn signal(&self) {
        {
            let _state = self.control.state.lock().unwrap();
            self.binding.revoke_admission();
            self.control.cancelled.store(true, Ordering::Release);
        }
        self.publisher.retire(&self.publication_owner);
        self.control.changed.notify_all();
    }
    fn start(&self, deadline: Instant) -> bool {
        let mut state = self.control.state.lock().unwrap();
        if self.control.cancelled.load(Ordering::Acquire)
            || state.started
            || Instant::now() >= deadline
        {
            return false;
        }
        state.started = true;
        true
    }
    fn wait_finished(&self) {
        let mut state = self.control.state.lock().unwrap();
        while state.started && !state.finished {
            state = self.control.changed.wait(state).unwrap();
        }
    }
}
struct Running(Arc<ActiveRun>);
impl Drop for Running {
    fn drop(&mut self) {
        let mut state = self.0.control.state.lock().unwrap();
        state.finished = true;
        self.0.control.changed.notify_all();
    }
}
struct Lane {
    invocation: Option<u64>,
    last_invocation: Option<u64>,
    sequence: u64,
    admission: Arc<()>,
    admission_epoch: Option<ManagementEpoch>,
    active: Option<Arc<ActiveRun>>,
}
impl Default for Lane {
    fn default() -> Self {
        Self {
            invocation: None,
            last_invocation: None,
            sequence: 0,
            admission: Arc::new(()),
            admission_epoch: None,
            active: None,
        }
    }
}
pub(crate) struct LauncherRuntime {
    management: Arc<ManagementState>,
    registry: ResultRegistry,
    publisher: Arc<MainResultPublisher>,
    transport: Option<Arc<dyn RuntimeTransport>>,
    registered: AtomicBool,
    lane: Mutex<Lane>,
    timeout: Duration,
    #[cfg(test)]
    before_open: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
static NEXT_REQUEST: AtomicU64 = AtomicU64::new(0);
impl LauncherRuntime {
    pub(crate) fn closed(management: Arc<ManagementState>, registry: ResultRegistry) -> Arc<Self> {
        Self::construct(management, registry, None, Duration::from_secs(30))
    }
    fn construct(
        management: Arc<ManagementState>,
        registry: ResultRegistry,
        transport: Option<Arc<dyn RuntimeTransport>>,
        timeout: Duration,
    ) -> Arc<Self> {
        let runtime = Arc::new(Self {
            management: management.clone(),
            publisher: Arc::new(MainResultPublisher::new(registry.clone())),
            registry,
            transport,
            registered: AtomicBool::new(false),
            lane: Mutex::new(Lane::default()),
            timeout,
            #[cfg(test)]
            before_open: Mutex::new(None),
        });
        let hook: Arc<dyn RuntimeInvalidation> = runtime.clone();
        runtime.registered.store(
            management.register_runtime(Arc::downgrade(&hook)),
            Ordering::Release,
        );
        runtime
    }
    #[cfg(test)]
    fn with_transport(
        management: Arc<ManagementState>,
        registry: ResultRegistry,
        transport: Arc<dyn RuntimeTransport>,
        timeout: Duration,
    ) -> Arc<Self> {
        Self::construct(management, registry, Some(transport), timeout)
    }
    pub(crate) fn available(&self) -> bool {
        self.transport.is_some() && self.registered.load(Ordering::Acquire)
    }
    pub(crate) fn begin_invocation(&self, id: u64) {
        let old = {
            let mut lane = self.lane.lock().unwrap();
            if lane.last_invocation.is_some_and(|last| id <= last) {
                return;
            }
            lane.last_invocation = Some(id);
            lane.invocation = Some(id);
            lane.sequence = 0;
            lane.admission = Arc::new(());
            lane.admission_epoch = None;
            lane.active.take()
        };
        self.retire(old);
    }
    /// Trusted host hide/exit only. Admission closes before return; asynchronous
    /// draining is not an acknowledgement that an immediately exiting OS process
    /// has already finished its cleanup.
    pub(crate) fn invalidate_current(&self) {
        self.invalidate(true);
    }
    fn invalidate(&self, hide: bool) {
        let old = {
            let mut lane = self.lane.lock().unwrap();
            if hide {
                lane.invocation = None;
            }
            lane.admission = Arc::new(());
            lane.admission_epoch = None;
            lane.active.take()
        };
        self.retire(old);
    }
    fn retire(&self, old: Option<Arc<ActiveRun>>) {
        if let Some(old) = old {
            old.signal();
            close_later(self.management.clone(), old.identity.clone());
        }
    }
    fn admit_query(
        &self,
        invocation: u64,
        sequence: u64,
        epoch: &ManagementEpoch,
    ) -> Option<Arc<()>> {
        let (admission, old) = {
            let mut lane = self.lane.lock().ok()?;
            if lane.invocation != Some(invocation)
                || !self.management.runtime_epoch_current(epoch)
                || sequence <= lane.sequence
                || !self.registry.can_begin_query(
                    QueryDomain::Plugin,
                    &invocation.to_string(),
                    sequence,
                )
            {
                return None;
            }
            lane.sequence = sequence;
            lane.admission = Arc::new(());
            lane.admission_epoch = Some(epoch.clone());
            (lane.admission.clone(), lane.active.take())
        };
        self.retire(old);
        Some(admission)
    }
    fn current(&self, admission: &Arc<()>, epoch: &ManagementEpoch) -> bool {
        let lane = self.lane.lock().unwrap();
        Arc::ptr_eq(&lane.admission, admission) && self.management.runtime_epoch_current(epoch)
    }

    pub(crate) fn copy_result(
        &self,
        appdata: &Path,
        request_id: &str,
        result_id: &str,
        copy: impl FnOnce(&str, &mut dyn FnMut() -> bool) -> Result<HostCopyOutcome, HostCopyError>,
    ) -> Result<MainActionCompletionOwner, HostCopyError> {
        let denied = HostCopyError::PermissionDenied;
        if !self.available() {
            return Err(denied);
        }
        let (action, ticket) = self
            .registry
            .resolve_with_ticket(request_id, result_id)
            .map_err(|_| denied)?;
        let ResultAction::CopyText {
            command_id: Some(_),
            plugin_id,
            generation,
            text,
        } = action
        else {
            return Err(denied);
        };
        let run = self
            .lane
            .lock()
            .map_err(|_| denied)?
            .active
            .clone()
            .ok_or(denied)?;
        let context = run.binding.context();
        if context.plugin_id != plugin_id || context.plugin_generation != generation {
            return Err(denied);
        }

        let mut completion = None;
        let mut admit = || {
            if completion.is_some() {
                return false;
            }
            completion = self
                .management
                .admit_clipboard_write(
                    appdata,
                    run.management_epoch.clone(),
                    &run.identity,
                    &plugin_id,
                    generation,
                    || {
                        let lane = self.lane.lock().ok()?;
                        let _control = run.control.state.lock().ok()?;
                        if run.control.cancelled.load(Ordering::Acquire)
                            || !lane
                                .active
                                .as_ref()
                                .is_some_and(|active| Arc::ptr_eq(active, &run))
                        {
                            return None;
                        }
                        self.registry
                            .consume_main_action(&ticket, MainActionKind::PluginCopyText)
                            .ok()
                    },
                )
                .ok();
            completion.is_some()
        };
        copy(&text, &mut admit)?;
        completion.ok_or(denied)
    }

    /// Blocking worker only. No management lock is held while transport.run or
    /// callbacks execute. Non-plugin queries still retire an old active run,
    /// without consuming the registry sequence needed by normal app search.
    pub(crate) fn route(
        &self,
        appdata: &Path,
        invocation: u64,
        sequence: u64,
        query: &str,
        theme: InvocationTheme,
        submit: bool,
        origin: Option<CompletionOrigin>,
    ) -> Result<Route, Error> {
        if !self.available() {
            return Ok(Route::NotHandled);
        }
        let epoch = self.management.epoch().map_err(|_| Error::Unavailable)?;
        let Some(admission) = self.admit_query(invocation, sequence, &epoch) else {
            return Ok(Route::Stale);
        };
        let Some((name, input)) = split_query(query) else {
            return Ok(if origin.is_some() {
                Route::Stale
            } else {
                Route::NotHandled
            });
        };

        let commands = match self.management.runtime_commands(
            appdata,
            &epoch,
            name,
            origin.is_none()
                && input.is_empty()
                && !submit
                && !host_command_names().any(|command| command.starts_with(name)),
        ) {
            Ok(commands) => commands,
            Err(super::management_macos::Error::Stale) => return Ok(Route::Stale),
            Err(_) => return Err(Error::Storage),
        };
        let Some(first) = commands.first() else {
            return Ok(if origin.is_some() {
                Route::Stale
            } else {
                Route::NotHandled
            });
        };
        if first.name != name {
            let items = commands
                .into_iter()
                .filter_map(|command| {
                    let title = format!("/{}", command.name);
                    let activation = LauncherResultActivation::main_result_activation(
                        command.plugin_id,
                        command.command_id,
                        command.name,
                        String::new(),
                        false,
                    )?;
                    Some(ResultItem {
                        result_actions: None,
                        result_id: String::new(),
                        activation,
                        title,
                        subtitle: command.summary,
                        detail: None,
                        icon: None,
                        plugin_icon_url: None,
                        icon_kind: None,
                        favorite: None,
                        has_default_action: false,
                    })
                })
                .collect();
            return Ok(self.publish_catalog(&admission, &epoch, invocation, sequence, items, None));
        }
        let command = commands.into_iter().next().expect("nonempty catalog");
        if let Some(origin) = &origin {
            if origin.plugin_id != command.plugin_id
                || origin.command_id.as_deref().unwrap_or("default") != command.command_id
                || (origin.phase == CompletionPhase::Commit) != submit
            {
                return Ok(Route::Stale);
            }
        }
        if origin
            .as_ref()
            .is_some_and(|origin| origin.phase == CompletionPhase::Preview)
            || (command.input_required && input.is_empty())
            || (command.mode == PublicActivationMode::Submit
                && !submit
                && command.input_placeholder.is_some())
        {
            return Ok(self.publish_catalog(
                &admission,
                &epoch,
                invocation,
                sequence,
                Vec::new(),
                command.input_placeholder,
            ));
        }
        if command.mode == PublicActivationMode::Submit && !submit {
            return Ok(Route::NotHandled);
        }
        if !self.current(&admission, &epoch) {
            return Ok(Route::Stale);
        }
        let number = NEXT_REQUEST
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::Unavailable)?
            + 1;
        let request_id = format!("public-request-{number:016x}");
        let deadline = Instant::now() + self.timeout;
        let identity = Arc::new(());
        let mut active = None;
        #[cfg(test)]
        {
            let hook = self.before_open.lock().unwrap().take();
            if let Some(hook) = hook {
                hook();
            }
        }
        let opened = self.management.open_runtime(
            appdata,
            &epoch,
            &command,
            identity.clone(),
            &request_id,
            deadline,
            || self.current(&admission, &epoch),
            |installed| {
                let mut lane = self.lane.lock().map_err(|_| LifecycleError::Runtime)?;
                if !Arc::ptr_eq(&lane.admission, &admission)
                    || !self.management.runtime_epoch_current(&epoch)
                {
                    return Err(LifecycleError::Stale);
                }
                let owner = self
                    .publisher
                    .begin(
                        installed.binding().clone(),
                        &invocation.to_string(),
                        sequence,
                        installed
                            .clipboard_write_allowed()
                            .then_some(installed.command_id()),
                    )
                    .ok_or(LifecycleError::Stale)?;
                let run = Arc::new(ActiveRun {
                    identity: identity.clone(),
                    management_epoch: epoch.clone(),
                    binding: installed.binding().clone(),
                    publisher: self.publisher.clone(),
                    publication_owner: Arc::new(owner),
                    control: Arc::new(Control::default()),
                });
                lane.active = Some(run.clone());
                active = Some(run.clone());
                Ok(Box::new(move || {
                    run.signal();
                    run.wait_finished();
                    Ok(())
                }))
            },
        );
        let installed = match opened {
            Ok(value) => value,
            Err(error) => {
                if let Some(run) = active {
                    run.signal();
                    close_later(self.management.clone(), run.identity.clone());
                }
                return if matches!(error, super::management_macos::Error::Stale) {
                    Ok(Route::Stale)
                } else {
                    Err(Error::Storage)
                };
            }
        };
        let run = active.ok_or(Error::Unavailable)?;
        if !self.current(&admission, &epoch) || !run.start(deadline) {
            run.signal();
            close_later(self.management.clone(), identity);
            return Ok(Route::Stale);
        }
        let running = Running(run.clone());
        if !watch_deadline(run.clone(), self.management.clone(), deadline) {
            drop(running);
            run.signal();
            close_later(self.management.clone(), identity);
            return Err(Error::Unavailable);
        }
        let command_context = submit.then(|| MainResultCommandContext {
            plugin_id: command.plugin_id.clone(),
            command_id: command.command_id.clone(),
            command_label: command.name.clone(),
            argument: input.into(),
        });
        let wire = PluginInvocation {
            api_version: 1,
            command_id: command.multi.then_some(command.command_id),
            request_id,
            input: input.into(),
            context: PluginInvocationEnvironment {
                platform: PluginInvocationPlatform::Macos,
                theme,
                invoked_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            },
        };
        let input = RuntimeInput {
            invocation: wire,
            package: installed.package().clone(),
            binding: installed.binding().clone(),
            deadline,
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.transport
                .as_ref()
                .unwrap()
                .run(input, Cancellation(run.control.clone()))
        }))
        .unwrap_or(Err(Error::Unavailable));
        // Lifecycle retirement may now drain, without waiting for publication or
        // requiring this worker to reacquire the management IO mutex.
        drop(running);
        if !self.current(&admission, &epoch) || run.control.cancelled.load(Ordering::Acquire) {
            return Ok(Route::Stale);
        }
        let terminal = match result {
            Ok(value) => value,
            Err(error) => {
                run.signal();
                close_later(self.management.clone(), identity);
                return Err(error);
            }
        };
        let prepared = match self
            .publisher
            .prepare(&run.publication_owner, &terminal.raw)
        {
            Ok(value) => value,
            Err(error) => {
                run.signal();
                close_later(self.management.clone(), identity);
                return completion_error(error);
            }
        };
        match self.publisher.publish(&run.publication_owner, prepared) {
            Ok(Some(mut response)) => {
                response.replace_local_results = true;
                response.main_result_command = command_context;
                Ok(Route::Published(response))
            }
            Ok(None) => Ok(Route::Stale),
            Err(error) => completion_error(error),
        }
    }

    fn publish_catalog(
        &self,
        admission: &Arc<()>,
        epoch: &ManagementEpoch,
        invocation: u64,
        sequence: u64,
        items: Vec<ResultItem>,
        hint: Option<String>,
    ) -> Route {
        let Ok(lane) = self.lane.lock() else {
            return Route::Stale;
        };
        if !Arc::ptr_eq(&lane.admission, admission) || !self.management.runtime_epoch_current(epoch)
        {
            return Route::Stale;
        }
        let Some(query) =
            self.registry
                .begin_query(QueryDomain::Plugin, &invocation.to_string(), sequence)
        else {
            return Route::Stale;
        };
        self.registry
            .publish_if_latest(
                query,
                items
                    .into_iter()
                    .map(|item| (item, None::<ResultAction>))
                    .collect(),
                || true,
                |id, items| {
                    let mut response = search_response(id, items);
                    response.replace_local_results = true;
                    response.command_hint = hint;
                    response
                },
            )
            .map_or(Route::Stale, Route::Published)
    }
}
impl RuntimeInvalidation for LauncherRuntime {
    fn invalidate_runtime(&self, retired: &ManagementEpoch) {
        let old = {
            let mut lane = self.lane.lock().unwrap();
            if lane
                .admission_epoch
                .as_ref()
                .is_some_and(|e| e.matches(retired))
            {
                lane.admission = Arc::new(());
                lane.admission_epoch = None;
            }
            if lane
                .active
                .as_ref()
                .is_some_and(|r| r.management_epoch.matches(retired))
            {
                lane.active.take()
            } else {
                None
            }
        };
        self.retire(old);
    }
}
impl Drop for LauncherRuntime {
    fn drop(&mut self) {
        self.invalidate_current();
    }
}
fn completion_error(error: MainCompletionError) -> Result<Route, Error> {
    match error {
        MainCompletionError::Expired => Ok(Route::Stale),
        MainCompletionError::UnsupportedAction => Err(Error::UnsupportedAction),
        MainCompletionError::InvalidResult => Err(Error::InvalidResult),
        _ => Err(Error::Unavailable),
    }
}
fn close_later(management: Arc<ManagementState>, owner: Arc<()>) {
    // Failure to create a cleanup thread never reopens admission. A subsequent
    // lifecycle operation still closes/drains the retained original request.
    let _ = std::thread::Builder::new()
        .name("plugin-runtime-retire".into())
        .spawn(move || {
            let _ = management.close_runtime(&owner);
        });
}
fn watch_deadline(
    run: Arc<ActiveRun>,
    management: Arc<ManagementState>,
    deadline: Instant,
) -> bool {
    std::thread::Builder::new()
        .name("plugin-runtime-deadline".into())
        .spawn(move || {
            let mut state = run.control.state.lock().unwrap();
            loop {
                if state.finished || run.control.cancelled.load(Ordering::Acquire) {
                    return;
                }
                let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                    break;
                };
                let (next, wait) = run.control.changed.wait_timeout(state, remaining).unwrap();
                state = next;
                if wait.timed_out() {
                    break;
                }
            }
            drop(state);
            run.signal();
            close_later(management, run.identity.clone());
        })
        .is_ok()
}
fn host_command_names() -> impl Iterator<Item = &'static str> {
    ["find", "settings", "plugins"].into_iter().chain(
        crate::settings::ManagedBuiltinFeature::ALL
            .into_iter()
            .map(crate::settings::ManagedBuiltinFeature::default_name),
    )
}

fn split_query(query: &str) -> Option<(&str, &str)> {
    let command = query.trim().strip_prefix('/')?;
    let (name, input) = command
        .split_once(' ')
        .map_or((command, ""), |(name, input)| (name, input.trim()));
    // Host-owned names always retain priority, including currently gated panels.
    if input.len() > 64 * 1024
        || query.contains('\0')
        || name.is_empty()
        || host_command_names().any(|command| command == name)
    {
        return None;
    }
    Some((name, input))
}
#[cfg(test)]
#[path = "launcher_runtime_macos_tests.rs"]
mod tests;
