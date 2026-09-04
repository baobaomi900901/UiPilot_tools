use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use everything_ipc::client::{EverythingClient, EverythingClientError};
use everything_ipc::protocol::{EverythingQueryResult, EverythingQuerySpec, EverythingSort};

use super::windows::everything_process::EverythingProcessCommand;

const PROBE_REQUEST_FLAGS: u32 = 0x155;
const PROBE_SORT_TYPE: u32 = 14;

pub(crate) const INSTANCE_NAME: &str = "UiPilot";
pub(crate) const SERVICE_NAME: &str = "Everything (UiPilot)";
pub(crate) const PIPE_NAME: &str = r"\\.\PIPE\UiPilotEverythingService";
const RESOURCE_SUBDIRECTORY: &str = r"resources\everything";
const EXECUTABLE_FILE_NAME: &str = "Everything.exe";
const LOCK_FILE_NAME: &str = "everything.lock.json";
const TEMPLATE_FILE_NAME: &str = "Everything-UiPilot.ini";
const REVIEWED_LOCK_BYTES: &[u8] =
    include_bytes!("../../resources/everything/everything.lock.json");
const REVIEWED_INI_BYTES: &[u8] =
    include_bytes!("../../resources/everything/Everything-UiPilot.ini");

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReviewedLock {
    schema_version: u32,
    pub(crate) version: String,
    pub(crate) architecture: String,
    artifact_file_name: String,
    source_url: String,
    artifact_sha256: String,
    pub(crate) everything_exe_sha256: String,
    pub(crate) authenticode_publisher: String,
    license: String,
    license_url: String,
    license_sha256: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeManifest {
    schema_version: u32,
    version: String,
    architecture: String,
    sha256: String,
    publisher: String,
    instance: String,
    service_name: String,
    pipe_name: String,
    desktop_user_sid: String,
    expanded_sddl: String,
    service_binary_path: PathBuf,
}

#[derive(Debug)]
pub(crate) struct RuntimeResourcePaths {
    pub(crate) executable: PathBuf,
    pub(crate) lock: PathBuf,
    pub(crate) template: PathBuf,
}

impl RuntimeResourcePaths {
    pub(crate) fn resolve(resource_directory: &Path) -> Self {
        let root = resource_directory.join(RESOURCE_SUBDIRECTORY);
        Self {
            executable: root.join(EXECUTABLE_FILE_NAME),
            lock: root.join(LOCK_FILE_NAME),
            template: root.join(TEMPLATE_FILE_NAME),
        }
    }
}

pub(crate) fn parse_reviewed_lock(bytes: &[u8]) -> Result<ReviewedLock, RuntimeUnavailableReason> {
    let lock: ReviewedLock =
        serde_json::from_slice(bytes).map_err(|_| RuntimeUnavailableReason::InvalidResource)?;
    let version_parts = lock.version.split('.').collect::<Vec<_>>();
    let canonical_version = version_parts.len() == 4
        && version_parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (*part == "0" || !part.starts_with('0'))
        });
    let valid_hex = |value: &str| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    };
    if lock.schema_version != 1
        || lock.architecture != "x64"
        || !canonical_version
        || !valid_hex(&lock.artifact_sha256)
        || !valid_hex(&lock.everything_exe_sha256)
        || !valid_hex(&lock.license_sha256)
        || lock.artifact_file_name != format!("Everything-{}.x64.zip", lock.version)
        || lock.source_url
            != format!(
                "https://www.voidtools.com/Everything-{}.x64.zip",
                lock.version
            )
        || lock.authenticode_publisher.is_empty()
        || lock.license != "MIT"
        || lock.license_url != "https://www.voidtools.com/License.txt"
    {
        return Err(RuntimeUnavailableReason::InvalidResource);
    }
    Ok(lock)
}

pub(crate) fn parse_runtime_manifest(
    bytes: &[u8],
    lock: &ReviewedLock,
) -> Result<RuntimeManifest, RuntimeUnavailableReason> {
    let manifest: RuntimeManifest =
        serde_json::from_slice(bytes).map_err(|_| RuntimeUnavailableReason::InvalidManifest)?;
    let sid_is_canonical = manifest.desktop_user_sid.starts_with("S-1-")
        && manifest.desktop_user_sid.len() <= 184
        && manifest.desktop_user_sid.split('-').skip(2).all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'))
        });
    let expected_sddl = format!(
        "D:P(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)(A;OICI;GRGW;;;{})",
        manifest.desktop_user_sid
    );
    if manifest.schema_version != 1
        || manifest.version != lock.version
        || manifest.architecture != lock.architecture
        || manifest.sha256 != lock.everything_exe_sha256
        || manifest.publisher != lock.authenticode_publisher
        || manifest.instance != INSTANCE_NAME
        || manifest.service_name != SERVICE_NAME
        || manifest.pipe_name != PIPE_NAME
        || !sid_is_canonical
        || manifest.expanded_sddl != expected_sddl
        || manifest
            .service_binary_path
            .file_name()
            .and_then(|name| name.to_str())
            != Some(EXECUTABLE_FILE_NAME)
    {
        return Err(RuntimeUnavailableReason::InvalidManifest);
    }
    Ok(manifest)
}

pub(crate) fn managed_ini_bytes(bytes: &[u8]) -> Result<Vec<u8>, RuntimeUnavailableReason> {
    if bytes != REVIEWED_INI_BYTES {
        return Err(RuntimeUnavailableReason::InvalidResource);
    }
    Ok(bytes.to_vec())
}

pub(crate) fn build_runtime_launch(
    executable: PathBuf,
    ini: PathBuf,
    database: PathBuf,
) -> Result<RuntimeLaunch, RuntimeUnavailableReason> {
    let arguments = vec![
        "-instance".into(),
        INSTANCE_NAME.into(),
        "-service-pipe-name".into(),
        PIPE_NAME.into(),
        "-config".into(),
        ini.into_os_string(),
        "-db".into(),
        database.into_os_string(),
        "-startup".into(),
    ];
    let command = EverythingProcessCommand::new(executable, arguments)
        .map_err(|_| RuntimeUnavailableReason::InvalidResource)?;
    Ok(RuntimeLaunch { command })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EverythingRuntimeStatus {
    Starting,
    Ready,
    Unavailable,
    Stopping,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeUnavailableReason {
    RuntimeAlreadyOwned,
    InvalidResource,
    InvalidManifest,
    ForeignProcess,
    ServiceUnavailable,
    ProcessLaunchFailed,
    IpcUnavailable,
    CapabilityMismatch,
    StartupTimedOut,
    ProcessWaitFailed,
    RetriesExhausted,
    GenerationExhausted,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EverythingRuntimeSnapshot {
    pub(crate) status: EverythingRuntimeStatus,
    pub(crate) reason: Option<RuntimeUnavailableReason>,
}

pub(crate) trait EverythingRuntimeClient: Send + Sync {
    fn database_loaded(&self, timeout: Duration) -> Result<bool, EverythingClientError>;
    fn query(
        &self,
        spec: EverythingQuerySpec,
    ) -> Result<EverythingQueryResult, EverythingClientError>;
}

impl EverythingRuntimeClient for EverythingClient {
    fn database_loaded(&self, timeout: Duration) -> Result<bool, EverythingClientError> {
        EverythingClient::database_loaded(self, timeout)
    }

    fn query(
        &self,
        spec: EverythingQuerySpec,
    ) -> Result<EverythingQueryResult, EverythingClientError> {
        EverythingClient::query(self, spec)
    }
}

#[derive(Clone)]
pub(crate) struct ReadyClientLease {
    generation: u64,
    client: Arc<dyn EverythingRuntimeClient>,
}

impl ReadyClientLease {
    pub(crate) fn new(generation: u64, client: Arc<dyn EverythingRuntimeClient>) -> Self {
        Self { generation, client }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn query(
        &self,
        spec: EverythingQuerySpec,
    ) -> Result<EverythingQueryResult, EverythingClientError> {
        self.client.query(spec)
    }
}

pub(crate) trait ReadyClientProvider: Send + Sync {
    fn ready_client(&self) -> Result<ReadyClientLease, EverythingRuntimeStatus>;
    fn invalidate_if_current(&self, generation: u64);
}

pub(crate) trait OwnedRuntimeProcess: Send {
    fn poll_exited(&mut self) -> Result<bool, RuntimeUnavailableReason>;
    fn shutdown(&mut self);
}

pub(crate) struct StartedRuntime {
    client: Arc<dyn EverythingRuntimeClient>,
    process: Box<dyn OwnedRuntimeProcess>,
}

impl StartedRuntime {
    pub(crate) fn new(
        client: Arc<dyn EverythingRuntimeClient>,
        process: Box<dyn OwnedRuntimeProcess>,
    ) -> Self {
        Self { client, process }
    }
}

pub(crate) trait RuntimeBackend: Send + Sync {
    fn start(
        &self,
        deadline: Instant,
        cancelled: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<StartedRuntime, RuntimeUnavailableReason>;
}

pub(crate) struct VerifiedResources {
    executable: PathBuf,
    lock: ReviewedLock,
    ini: Vec<u8>,
}

impl VerifiedResources {
    #[cfg(test)]
    fn for_test() -> Self {
        Self {
            executable: PathBuf::from(r"C:\UiPilot\Everything.exe"),
            lock: parse_reviewed_lock(REVIEWED_LOCK_BYTES).expect("reviewed lock must parse"),
            ini: REVIEWED_INI_BYTES.to_vec(),
        }
    }
}

pub(crate) struct ValidatedManifest {
    manifest: RuntimeManifest,
}

impl ValidatedManifest {
    #[cfg(test)]
    fn for_test() -> Self {
        let resources = VerifiedResources::for_test();
        let bytes = format!(
            r#"{{"schema_version":1,"version":"{}","architecture":"x64","sha256":"{}","publisher":{},"instance":"{}","service_name":"{}","pipe_name":{},"desktop_user_sid":"S-1-5-21-1-2-3-1001","expanded_sddl":"D:P(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)(A;OICI;GRGW;;;S-1-5-21-1-2-3-1001)","service_binary_path":"C:\\ProgramData\\UiPilot_tools\\everything\\{}\\Everything.exe"}}"#,
            resources.lock.version,
            resources.lock.everything_exe_sha256,
            serde_json::to_string(&resources.lock.authenticode_publisher).unwrap(),
            INSTANCE_NAME,
            SERVICE_NAME,
            serde_json::to_string(PIPE_NAME).unwrap(),
            resources.lock.version,
        );
        Self {
            manifest: parse_runtime_manifest(bytes.as_bytes(), &resources.lock).unwrap(),
        }
    }
}

pub(crate) struct RuntimeLaunch {
    command: EverythingProcessCommand,
}

impl RuntimeLaunch {
    #[cfg(test)]
    fn for_test() -> Self {
        Self {
            command: EverythingProcessCommand::new(
                std::path::PathBuf::from(r"C:\UiPilot\Everything.exe"),
                vec!["-startup".into()],
            )
            .unwrap(),
        }
    }
}

pub(crate) trait RuntimeSystem: Send + Sync {
    type Guard: Send + 'static;
    type Process: OwnedRuntimeProcess + 'static;

    fn verify_resources(&self) -> Result<VerifiedResources, RuntimeUnavailableReason>;
    fn verify_manifest(
        &self,
        resources: &VerifiedResources,
    ) -> Result<ValidatedManifest, RuntimeUnavailableReason>;
    fn acquire_runtime_mutex(&self) -> Result<Self::Guard, RuntimeUnavailableReason>;
    fn private_window_pid(&self) -> Result<Option<u32>, RuntimeUnavailableReason>;
    fn orphan_is_managed(
        &self,
        pid: u32,
        resources: &VerifiedResources,
    ) -> Result<bool, RuntimeUnavailableReason>;
    fn stop_orphan(&self, pid: u32, deadline: Instant) -> Result<(), RuntimeUnavailableReason>;
    fn prepare_launch(
        &self,
        resources: &VerifiedResources,
    ) -> Result<RuntimeLaunch, RuntimeUnavailableReason>;
    fn service_is_running(
        &self,
        manifest: &ValidatedManifest,
    ) -> Result<bool, RuntimeUnavailableReason>;
    fn launch_process(
        &self,
        launch: RuntimeLaunch,
    ) -> Result<Self::Process, RuntimeUnavailableReason>;
    fn connect_owned(
        &self,
        pid: u32,
        timeout: Duration,
    ) -> Result<Arc<dyn EverythingRuntimeClient>, RuntimeUnavailableReason>;
    fn process_pid(&self, process: &Self::Process) -> u32;
}

struct OwnedRuntimeSession<P: OwnedRuntimeProcess, G: Send> {
    process: P,
    _guard: G,
}

impl<P: OwnedRuntimeProcess, G: Send> OwnedRuntimeProcess for OwnedRuntimeSession<P, G> {
    fn poll_exited(&mut self) -> Result<bool, RuntimeUnavailableReason> {
        self.process.poll_exited()
    }

    fn shutdown(&mut self) {
        self.process.shutdown();
    }
}

pub(crate) fn start_runtime_with<S: RuntimeSystem>(
    system: &S,
    deadline: Instant,
    cancelled: &(dyn Fn() -> bool + Send + Sync),
) -> Result<StartedRuntime, RuntimeUnavailableReason> {
    ensure_start_continues(deadline, cancelled)?;
    let resources = system.verify_resources()?;
    let manifest = system.verify_manifest(&resources)?;
    let guard = system.acquire_runtime_mutex()?;
    ensure_start_continues(deadline, cancelled)?;
    if let Some(pid) = system.private_window_pid()? {
        if !system.orphan_is_managed(pid, &resources)? {
            return Err(RuntimeUnavailableReason::ForeignProcess);
        }
        system.stop_orphan(pid, deadline)?;
    }
    ensure_start_continues(deadline, cancelled)?;
    let launch = system.prepare_launch(&resources)?;
    if !system.service_is_running(&manifest)? {
        return Err(RuntimeUnavailableReason::ServiceUnavailable);
    }
    let mut process = system.launch_process(launch)?;
    let pid = system.process_pid(&process);
    let client = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if cancelled() || remaining.is_zero() {
            process.shutdown();
            return Err(RuntimeUnavailableReason::StartupTimedOut);
        }
        match process.poll_exited() {
            Ok(false) => {}
            Ok(true) => {
                process.shutdown();
                return Err(RuntimeUnavailableReason::ProcessLaunchFailed);
            }
            Err(error) => {
                process.shutdown();
                return Err(error);
            }
        }
        match system.connect_owned(pid, remaining.min(Duration::from_millis(250))) {
            Ok(client) => break client,
            Err(RuntimeUnavailableReason::IpcUnavailable) => {
                std::thread::sleep(
                    Duration::from_millis(10)
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            Err(error) => {
                process.shutdown();
                return Err(error);
            }
        }
    };
    Ok(StartedRuntime::new(
        client,
        Box::new(OwnedRuntimeSession {
            process,
            _guard: guard,
        }),
    ))
}

fn ensure_start_continues(
    deadline: Instant,
    cancelled: &(dyn Fn() -> bool + Send + Sync),
) -> Result<(), RuntimeUnavailableReason> {
    if cancelled() || Instant::now() >= deadline {
        Err(RuntimeUnavailableReason::StartupTimedOut)
    } else {
        Ok(())
    }
}

struct RuntimeState {
    status: EverythingRuntimeStatus,
    reason: Option<RuntimeUnavailableReason>,
    generation: u64,
    client: Option<Arc<dyn EverythingRuntimeClient>>,
    restart_requested: Option<u64>,
    shutdown: bool,
}

struct RuntimeInner {
    state: Mutex<RuntimeState>,
    changed: Condvar,
}

pub(crate) struct EverythingRuntimeSupervisor {
    inner: Arc<RuntimeInner>,
    backend: Arc<dyn RuntimeBackend>,
    readiness_timeout: Duration,
    poll_interval: Duration,
    restart_backoffs: [Duration; 3],
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl EverythingRuntimeSupervisor {
    pub(crate) fn production(resource_directory: PathBuf) -> Self {
        Self::with_backend(
            Arc::new(ProductionRuntimeBackend::new(resource_directory)),
            Duration::from_secs(60),
            Duration::from_millis(100),
            [
                Duration::from_millis(250),
                Duration::from_millis(500),
                Duration::from_secs(1),
            ],
        )
    }

    pub(crate) fn with_backend(
        backend: Arc<dyn RuntimeBackend>,
        readiness_timeout: Duration,
        poll_interval: Duration,
        restart_backoffs: [Duration; 3],
    ) -> Self {
        Self {
            inner: Arc::new(RuntimeInner {
                state: Mutex::new(RuntimeState {
                    status: EverythingRuntimeStatus::Starting,
                    reason: None,
                    generation: 0,
                    client: None,
                    restart_requested: None,
                    shutdown: false,
                }),
                changed: Condvar::new(),
            }),
            backend,
            readiness_timeout,
            poll_interval,
            restart_backoffs,
            worker: Mutex::new(None),
        }
    }

    pub(crate) fn start_async(&self) {
        let mut worker = self.worker.lock().expect("runtime worker mutex poisoned");
        if worker.is_some() {
            return;
        }
        if self
            .inner
            .state
            .lock()
            .expect("runtime state poisoned")
            .shutdown
        {
            return;
        }
        let inner = Arc::clone(&self.inner);
        let backend = Arc::clone(&self.backend);
        let readiness_timeout = self.readiness_timeout;
        let poll_interval = self.poll_interval;
        let restart_backoffs = self.restart_backoffs;
        *worker = Some(std::thread::spawn(move || {
            run_supervisor_worker(
                inner,
                backend,
                readiness_timeout,
                poll_interval,
                restart_backoffs,
            );
        }));
    }

    #[cfg(test)]
    pub(crate) fn snapshot(&self) -> EverythingRuntimeSnapshot {
        let state = self.inner.state.lock().expect("runtime state poisoned");
        EverythingRuntimeSnapshot {
            status: state.status,
            reason: state.reason,
        }
    }

    pub(crate) fn shutdown(&self) {
        {
            let mut state = self.inner.state.lock().expect("runtime state poisoned");
            state.shutdown = true;
            state.status = EverythingRuntimeStatus::Stopping;
            state.reason = None;
            state.client = None;
            state.restart_requested = None;
            self.inner.changed.notify_all();
        }
        if let Some(worker) = self
            .worker
            .lock()
            .expect("runtime worker mutex poisoned")
            .take()
        {
            let _ = worker.join();
        }
    }
}

struct ProductionRuntimeBackend {
    system: ProductionRuntimeSystem,
}

impl ProductionRuntimeBackend {
    fn new(resource_directory: PathBuf) -> Self {
        Self {
            system: ProductionRuntimeSystem::new(resource_directory),
        }
    }
}

impl RuntimeBackend for ProductionRuntimeBackend {
    fn start(
        &self,
        deadline: Instant,
        cancelled: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<StartedRuntime, RuntimeUnavailableReason> {
        start_runtime_with(&self.system, deadline, cancelled)
    }
}

#[cfg(windows)]
use windows_runtime::WindowsRuntimeSystem as ProductionRuntimeSystem;

#[cfg(not(windows))]
struct ProductionRuntimeSystem;

#[cfg(not(windows))]
impl ProductionRuntimeSystem {
    fn new(_resource_directory: PathBuf) -> Self {
        Self
    }
}

#[cfg(not(windows))]
impl RuntimeSystem for ProductionRuntimeSystem {
    type Guard = ();
    type Process = super::windows::everything_process::EverythingProcess;

    fn verify_resources(&self) -> Result<VerifiedResources, RuntimeUnavailableReason> {
        Err(RuntimeUnavailableReason::InvalidResource)
    }
    fn verify_manifest(
        &self,
        _: &VerifiedResources,
    ) -> Result<ValidatedManifest, RuntimeUnavailableReason> {
        Err(RuntimeUnavailableReason::InvalidManifest)
    }
    fn acquire_runtime_mutex(&self) -> Result<Self::Guard, RuntimeUnavailableReason> {
        Err(RuntimeUnavailableReason::RuntimeAlreadyOwned)
    }
    fn private_window_pid(&self) -> Result<Option<u32>, RuntimeUnavailableReason> {
        Ok(None)
    }
    fn orphan_is_managed(
        &self,
        _: u32,
        _: &VerifiedResources,
    ) -> Result<bool, RuntimeUnavailableReason> {
        Ok(false)
    }
    fn stop_orphan(&self, _: u32, _: Instant) -> Result<(), RuntimeUnavailableReason> {
        Err(RuntimeUnavailableReason::ForeignProcess)
    }
    fn prepare_launch(
        &self,
        _: &VerifiedResources,
    ) -> Result<RuntimeLaunch, RuntimeUnavailableReason> {
        Err(RuntimeUnavailableReason::InvalidResource)
    }
    fn service_is_running(&self, _: &ValidatedManifest) -> Result<bool, RuntimeUnavailableReason> {
        Ok(false)
    }
    fn launch_process(&self, _: RuntimeLaunch) -> Result<Self::Process, RuntimeUnavailableReason> {
        Err(RuntimeUnavailableReason::ProcessLaunchFailed)
    }
    fn connect_owned(
        &self,
        _: u32,
        _: Duration,
    ) -> Result<Arc<dyn EverythingRuntimeClient>, RuntimeUnavailableReason> {
        Err(RuntimeUnavailableReason::IpcUnavailable)
    }
    fn process_pid(&self, process: &Self::Process) -> u32 {
        process.pid()
    }
}

impl ReadyClientProvider for EverythingRuntimeSupervisor {
    fn ready_client(&self) -> Result<ReadyClientLease, EverythingRuntimeStatus> {
        let state = self.inner.state.lock().expect("runtime state poisoned");
        if state.status != EverythingRuntimeStatus::Ready {
            return Err(state.status);
        }
        let client = state
            .client
            .as_ref()
            .cloned()
            .ok_or(EverythingRuntimeStatus::Unavailable)?;
        Ok(ReadyClientLease::new(state.generation, client))
    }

    fn invalidate_if_current(&self, generation: u64) {
        let mut state = self.inner.state.lock().expect("runtime state poisoned");
        if !state.shutdown
            && state.status == EverythingRuntimeStatus::Ready
            && state.generation == generation
        {
            state.status = EverythingRuntimeStatus::Starting;
            state.client = None;
            state.restart_requested = Some(generation);
            self.inner.changed.notify_all();
        }
    }
}

enum MonitorOutcome {
    Restart,
    Stop,
    Terminal(RuntimeUnavailableReason),
}

fn run_supervisor_worker(
    inner: Arc<RuntimeInner>,
    backend: Arc<dyn RuntimeBackend>,
    readiness_timeout: Duration,
    poll_interval: Duration,
    restart_backoffs: [Duration; 3],
) {
    let mut retries_used = 0usize;
    loop {
        let generation = {
            let mut state = inner.state.lock().expect("runtime state poisoned");
            if state.shutdown {
                return;
            }
            let Some(generation) = state.generation.checked_add(1) else {
                publish_unavailable(&mut state, RuntimeUnavailableReason::GenerationExhausted);
                return;
            };
            state.generation = generation;
            state.status = EverythingRuntimeStatus::Starting;
            state.reason = None;
            state.client = None;
            state.restart_requested = None;
            generation
        };
        let deadline = Instant::now()
            .checked_add(readiness_timeout)
            .unwrap_or_else(Instant::now);
        let cancelled_inner = Arc::clone(&inner);
        let cancelled = move || {
            cancelled_inner
                .state
                .lock()
                .map(|state| state.shutdown)
                .unwrap_or(true)
        };
        let mut runtime = match backend.start(deadline, &cancelled) {
            Ok(runtime) => runtime,
            Err(reason) => {
                let mut state = inner.state.lock().expect("runtime state poisoned");
                if state.shutdown {
                    state.status = EverythingRuntimeStatus::Stopping;
                } else {
                    publish_unavailable(&mut state, reason);
                }
                return;
            }
        };

        match await_readiness(&inner, &mut runtime, deadline, poll_interval) {
            Ok(()) => {
                let mut state = inner.state.lock().expect("runtime state poisoned");
                if state.shutdown {
                    drop(state);
                    runtime.process.shutdown();
                    return;
                }
                state.status = EverythingRuntimeStatus::Ready;
                state.reason = None;
                state.client = Some(Arc::clone(&runtime.client));
            }
            Err(reason) => {
                runtime.process.shutdown();
                let mut state = inner.state.lock().expect("runtime state poisoned");
                if state.shutdown {
                    state.status = EverythingRuntimeStatus::Stopping;
                } else {
                    publish_unavailable(&mut state, reason);
                }
                return;
            }
        }

        match monitor_runtime(&inner, &mut runtime, generation, poll_interval) {
            MonitorOutcome::Stop => return,
            MonitorOutcome::Terminal(reason) => {
                let mut state = inner.state.lock().expect("runtime state poisoned");
                publish_unavailable(&mut state, reason);
                return;
            }
            MonitorOutcome::Restart => {
                if retries_used == restart_backoffs.len() {
                    let mut state = inner.state.lock().expect("runtime state poisoned");
                    publish_unavailable(&mut state, RuntimeUnavailableReason::RetriesExhausted);
                    return;
                }
                let backoff = restart_backoffs[retries_used];
                retries_used += 1;
                let state = inner.state.lock().expect("runtime state poisoned");
                let (state, _) = inner
                    .changed
                    .wait_timeout_while(state, backoff, |state| !state.shutdown)
                    .expect("runtime state poisoned");
                if state.shutdown {
                    return;
                }
            }
        }
    }
}

fn await_readiness(
    inner: &RuntimeInner,
    runtime: &mut StartedRuntime,
    deadline: Instant,
    poll_interval: Duration,
) -> Result<(), RuntimeUnavailableReason> {
    loop {
        if inner.state.lock().expect("runtime state poisoned").shutdown {
            return Err(RuntimeUnavailableReason::StartupTimedOut);
        }
        if runtime.process.poll_exited()? {
            return Err(RuntimeUnavailableReason::ProcessLaunchFailed);
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(RuntimeUnavailableReason::StartupTimedOut);
        }
        let operation_timeout = deadline
            .saturating_duration_since(now)
            .min(Duration::from_secs(1));
        match runtime.client.database_loaded(operation_timeout) {
            Ok(true) => break,
            Ok(false) => std::thread::sleep(
                poll_interval.min(deadline.saturating_duration_since(Instant::now())),
            ),
            Err(_) => return Err(RuntimeUnavailableReason::IpcUnavailable),
        }
    }
    let query_deadline = Instant::now()
        .checked_add(Duration::from_secs(1))
        .map(|value| value.min(deadline))
        .ok_or(RuntimeUnavailableReason::StartupTimedOut)?;
    let result = runtime
        .client
        .query(EverythingQuerySpec {
            search: Vec::new(),
            offset: 0,
            max_results: 1,
            request_flags: PROBE_REQUEST_FLAGS,
            sort: EverythingSort::DateModifiedDescending,
            deadline: query_deadline,
        })
        .map_err(|_| RuntimeUnavailableReason::IpcUnavailable)?;
    if result.request_flags != PROBE_REQUEST_FLAGS || result.sort_type != PROBE_SORT_TYPE {
        return Err(RuntimeUnavailableReason::CapabilityMismatch);
    }
    Ok(())
}

fn monitor_runtime(
    inner: &RuntimeInner,
    runtime: &mut StartedRuntime,
    generation: u64,
    poll_interval: Duration,
) -> MonitorOutcome {
    loop {
        {
            let mut state = inner.state.lock().expect("runtime state poisoned");
            if state.shutdown {
                state.status = EverythingRuntimeStatus::Stopping;
                state.client = None;
                drop(state);
                runtime.process.shutdown();
                return MonitorOutcome::Stop;
            }
            if state.restart_requested == Some(generation) {
                state.restart_requested = None;
                state.client = None;
                drop(state);
                runtime.process.shutdown();
                return MonitorOutcome::Restart;
            }
        }
        match runtime.process.poll_exited() {
            Ok(false) => {}
            Ok(true) => {
                let mut state = inner.state.lock().expect("runtime state poisoned");
                if state.generation == generation {
                    state.status = EverythingRuntimeStatus::Starting;
                    state.client = None;
                }
                return MonitorOutcome::Restart;
            }
            Err(reason) => {
                runtime.process.shutdown();
                return MonitorOutcome::Terminal(reason);
            }
        }
        let state = inner.state.lock().expect("runtime state poisoned");
        let _ = inner
            .changed
            .wait_timeout(state, poll_interval)
            .expect("runtime state poisoned");
    }
}

fn publish_unavailable(state: &mut RuntimeState, reason: RuntimeUnavailableReason) {
    state.status = EverythingRuntimeStatus::Unavailable;
    state.reason = Some(reason);
    state.client = None;
    state.restart_requested = None;
}

#[cfg(windows)]
mod windows_runtime {
    use std::ffi::{c_void, OsStr};
    use std::fs;
    use std::io::Read;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::ptr;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use sha2::{Digest, Sha256};
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HLOCAL, HWND, LPARAM, WAIT_EVENT,
        WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM,
    };
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows::Win32::Security::Cryptography::{
        CertCloseStore, CertFindCertificateInStore, CertFreeCertificateContext, CertStrToNameW,
        CryptMsgClose, CryptMsgGetParam, CryptQueryObject, CERT_FIND_SUBJECT_CERT,
        CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED, CERT_QUERY_FORMAT_FLAG_BINARY,
        CERT_QUERY_OBJECT_FILE, CERT_X500_NAME_STR, CMSG_SIGNER_CERT_INFO_PARAM, HCERTSTORE,
        PKCS_7_ASN_ENCODING, X509_ASN_ENCODING,
    };
    use windows::Win32::Security::WinTrust::{
        WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0,
        WINTRUST_FILE_INFO, WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT,
        WTD_REVOKE_WHOLECHAIN, WTD_STATEACTION_IGNORE, WTD_UI_NONE,
    };
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TokenUser, TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER,
    };
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
    };
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceConfigW,
        QueryServiceStatusEx, QUERY_SERVICE_CONFIGW, SC_MANAGER_CONNECT, SC_STATUS_PROCESS_INFO,
        SERVICE_QUERY_CONFIG, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_STATUS_PROCESS,
    };
    use windows::Win32::System::Threading::{
        CreateMutexW, GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken,
        QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject, PROCESS_ACCESS_RIGHTS,
        PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    };
    use windows::Win32::UI::Shell::{
        FOLDERID_LocalAppData, FOLDERID_ProgramData, SHGetKnownFolderPath, KF_FLAG_DEFAULT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetWindowThreadProcessId, SendMessageTimeoutW, SMTO_ABORTIFHUNG,
        SMTO_ERRORONEXIT, WM_USER,
    };

    use super::*;
    use crate::atomic_file;
    use crate::file_search::windows::everything_process::{self, EverythingProcess};

    const RUNTIME_MUTEX_NAME: &str = r"Local\UiPilotEverythingRuntime";
    const PRIVATE_WINDOW_CLASS: &str = "EVERYTHING_TASKBAR_NOTIFICATION_(UiPilot)";
    const MANAGED_RELATIVE_ROOT: &str = r"UiPilot_tools\everything";
    const RUNTIME_MANIFEST_NAME: &str = "managed-runtime.json";
    const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
    const SYNCHRONIZE_ACCESS: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(0x0010_0000);
    const ORPHAN_STOP_WAIT: Duration = Duration::from_secs(2);

    pub(super) struct WindowsRuntimeSystem {
        resource_directory: PathBuf,
        orphan: Mutex<Option<(u32, OwnedHandle)>>,
    }

    impl WindowsRuntimeSystem {
        pub(super) fn new(resource_directory: PathBuf) -> Self {
            Self {
                resource_directory,
                orphan: Mutex::new(None),
            }
        }
    }

    struct OwnedHandle(HANDLE);

    unsafe impl Send for OwnedHandle {}

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    pub(super) struct RuntimeMutex {
        _handle: OwnedHandle,
    }

    unsafe impl Send for RuntimeMutex {}

    impl OwnedRuntimeProcess for EverythingProcess {
        fn poll_exited(&mut self) -> Result<bool, RuntimeUnavailableReason> {
            EverythingProcess::poll_exited(self)
                .map_err(|_| RuntimeUnavailableReason::ProcessWaitFailed)
        }

        fn shutdown(&mut self) {
            EverythingProcess::shutdown(self);
        }
    }

    impl RuntimeSystem for WindowsRuntimeSystem {
        type Guard = RuntimeMutex;
        type Process = EverythingProcess;

        fn verify_resources(&self) -> Result<VerifiedResources, RuntimeUnavailableReason> {
            let root = fs::canonicalize(&self.resource_directory)
                .map_err(|_| RuntimeUnavailableReason::InvalidResource)?;
            let paths = RuntimeResourcePaths::resolve(&root);
            let resource_root = fs::canonicalize(root.join(RESOURCE_SUBDIRECTORY))
                .map_err(|_| RuntimeUnavailableReason::InvalidResource)?;
            let executable = canonical_regular_file(&paths.executable, &resource_root)?;
            let lock_path = canonical_regular_file(&paths.lock, &resource_root)?;
            let template_path = canonical_regular_file(&paths.template, &resource_root)?;
            let lock_bytes = bounded_read(&lock_path, MAX_MANIFEST_BYTES)
                .map_err(|_| RuntimeUnavailableReason::InvalidResource)?;
            if lock_bytes != REVIEWED_LOCK_BYTES {
                return Err(RuntimeUnavailableReason::InvalidResource);
            }
            let lock = parse_reviewed_lock(&lock_bytes)?;
            let ini = managed_ini_bytes(
                &bounded_read(&template_path, MAX_MANIFEST_BYTES)
                    .map_err(|_| RuntimeUnavailableReason::InvalidResource)?,
            )?;
            verify_binary_identity(
                &executable,
                &lock.everything_exe_sha256,
                &lock.version,
                &lock.authenticode_publisher,
            )
            .map_err(|_| RuntimeUnavailableReason::InvalidResource)?;
            Ok(VerifiedResources {
                executable,
                lock,
                ini,
            })
        }

        fn verify_manifest(
            &self,
            resources: &VerifiedResources,
        ) -> Result<ValidatedManifest, RuntimeUnavailableReason> {
            let program_data = known_folder(&FOLDERID_ProgramData)
                .map_err(|_| RuntimeUnavailableReason::InvalidManifest)?;
            let managed_root = fs::canonicalize(program_data.join(MANAGED_RELATIVE_ROOT))
                .map_err(|_| RuntimeUnavailableReason::InvalidManifest)?;
            let manifest_path =
                canonical_regular_file(&managed_root.join(RUNTIME_MANIFEST_NAME), &managed_root)
                    .map_err(|_| RuntimeUnavailableReason::InvalidManifest)?;
            let bytes = bounded_read(&manifest_path, MAX_MANIFEST_BYTES)
                .map_err(|_| RuntimeUnavailableReason::InvalidManifest)?;
            let manifest = parse_runtime_manifest(&bytes, &resources.lock)?;
            if current_user_sid().map_err(|_| RuntimeUnavailableReason::InvalidManifest)?
                != manifest.desktop_user_sid
            {
                return Err(RuntimeUnavailableReason::InvalidManifest);
            }
            let expected = managed_root
                .join(&manifest.version)
                .join(EXECUTABLE_FILE_NAME);
            let binary = canonical_regular_file(&manifest.service_binary_path, &managed_root)
                .map_err(|_| RuntimeUnavailableReason::InvalidManifest)?;
            let expected = fs::canonicalize(expected)
                .map_err(|_| RuntimeUnavailableReason::InvalidManifest)?;
            if binary != expected {
                return Err(RuntimeUnavailableReason::InvalidManifest);
            }
            verify_binary_identity(
                &binary,
                &manifest.sha256,
                &manifest.version,
                &manifest.publisher,
            )
            .map_err(|_| RuntimeUnavailableReason::InvalidManifest)?;
            Ok(ValidatedManifest { manifest })
        }

        fn acquire_runtime_mutex(&self) -> Result<Self::Guard, RuntimeUnavailableReason> {
            let name = wide(OsStr::new(RUNTIME_MUTEX_NAME));
            let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }
                .map_err(|_| RuntimeUnavailableReason::RuntimeAlreadyOwned)?;
            if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                let _ = unsafe { CloseHandle(handle) };
                return Err(RuntimeUnavailableReason::RuntimeAlreadyOwned);
            }
            Ok(RuntimeMutex {
                _handle: OwnedHandle(handle),
            })
        }

        fn private_window_pid(&self) -> Result<Option<u32>, RuntimeUnavailableReason> {
            private_window().map(|value| value.map(|(_, pid)| pid))
        }

        fn orphan_is_managed(
            &self,
            pid: u32,
            resources: &VerifiedResources,
        ) -> Result<bool, RuntimeUnavailableReason> {
            let rights = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | SYNCHRONIZE_ACCESS;
            let handle = unsafe { OpenProcess(rights, false, pid) }
                .map_err(|_| RuntimeUnavailableReason::ForeignProcess)?;
            let handle = OwnedHandle(handle);
            let path = process_image_path(handle.0)
                .and_then(|path| fs::canonicalize(path).map_err(|_| ()))
                .map_err(|_| RuntimeUnavailableReason::ForeignProcess)?;
            if path != resources.executable
                || verify_binary_identity(
                    &path,
                    &resources.lock.everything_exe_sha256,
                    &resources.lock.version,
                    &resources.lock.authenticode_publisher,
                )
                .is_err()
            {
                return Ok(false);
            }
            *self.orphan.lock().expect("orphan handle mutex poisoned") = Some((pid, handle));
            Ok(true)
        }

        fn stop_orphan(&self, pid: u32, deadline: Instant) -> Result<(), RuntimeUnavailableReason> {
            let Some((held_pid, handle)) = self
                .orphan
                .lock()
                .expect("orphan handle mutex poisoned")
                .take()
            else {
                return Err(RuntimeUnavailableReason::ForeignProcess);
            };
            if held_pid != pid {
                return Err(RuntimeUnavailableReason::ForeignProcess);
            }
            if let Some((window, actual_pid)) = private_window()? {
                if actual_pid != pid {
                    return Err(RuntimeUnavailableReason::ForeignProcess);
                }
                let _ = unsafe {
                    SendMessageTimeoutW(
                        window,
                        WM_USER,
                        WPARAM(4),
                        LPARAM(0),
                        SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
                        1_000,
                        None,
                    )
                };
            }
            let wait = ORPHAN_STOP_WAIT.min(deadline.saturating_duration_since(Instant::now()));
            if wait_handle(handle.0, wait)? == WAIT_TIMEOUT {
                unsafe { TerminateProcess(handle.0, 1) }
                    .map_err(|_| RuntimeUnavailableReason::ForeignProcess)?;
                if wait_handle(handle.0, ORPHAN_STOP_WAIT)? != WAIT_OBJECT_0 {
                    return Err(RuntimeUnavailableReason::ForeignProcess);
                }
            }
            Ok(())
        }

        fn prepare_launch(
            &self,
            resources: &VerifiedResources,
        ) -> Result<RuntimeLaunch, RuntimeUnavailableReason> {
            ensure_current_process_not_elevated()?;
            let mut session_id = 0u32;
            unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session_id) }
                .map_err(|_| RuntimeUnavailableReason::ProcessLaunchFailed)?;
            let local = known_folder(&FOLDERID_LocalAppData)
                .map_err(|_| RuntimeUnavailableReason::ProcessLaunchFailed)?;
            let local = fs::canonicalize(local)
                .map_err(|_| RuntimeUnavailableReason::ProcessLaunchFailed)?;
            let session_root = local
                .join(MANAGED_RELATIVE_ROOT)
                .join(session_id.to_string());
            fs::create_dir_all(&session_root)
                .map_err(|_| RuntimeUnavailableReason::ProcessLaunchFailed)?;
            let session_root = fs::canonicalize(session_root)
                .map_err(|_| RuntimeUnavailableReason::ProcessLaunchFailed)?;
            if !session_root.starts_with(&local) {
                return Err(RuntimeUnavailableReason::ProcessLaunchFailed);
            }
            let ini = session_root.join(TEMPLATE_FILE_NAME);
            let database = session_root.join("Everything-UiPilot.db");
            atomic_file::replace_current(&ini, &resources.ini)
                .map_err(|_| RuntimeUnavailableReason::ProcessLaunchFailed)?;
            build_runtime_launch(resources.executable.clone(), ini, database)
        }

        fn service_is_running(
            &self,
            manifest: &ValidatedManifest,
        ) -> Result<bool, RuntimeUnavailableReason> {
            query_private_service(&manifest.manifest)
        }

        fn launch_process(
            &self,
            launch: RuntimeLaunch,
        ) -> Result<Self::Process, RuntimeUnavailableReason> {
            everything_process::launch(&launch.command)
                .map_err(|_| RuntimeUnavailableReason::ProcessLaunchFailed)
        }

        fn connect_owned(
            &self,
            pid: u32,
            timeout: Duration,
        ) -> Result<Arc<dyn EverythingRuntimeClient>, RuntimeUnavailableReason> {
            EverythingClient::connect_owned(INSTANCE_NAME, pid, timeout)
                .map(|client| Arc::new(client) as Arc<dyn EverythingRuntimeClient>)
                .map_err(|error| match error {
                    EverythingClientError::IpcOwnerMismatch => {
                        RuntimeUnavailableReason::ForeignProcess
                    }
                    EverythingClientError::InvalidInstance => {
                        RuntimeUnavailableReason::InvalidResource
                    }
                    _ => RuntimeUnavailableReason::IpcUnavailable,
                })
        }

        fn process_pid(&self, process: &Self::Process) -> u32 {
            process.pid()
        }
    }

    fn bounded_read(path: &Path, maximum: u64) -> Result<Vec<u8>, ()> {
        let metadata = fs::metadata(path).map_err(|_| ())?;
        if !metadata.is_file() || metadata.len() > maximum {
            return Err(());
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        fs::File::open(path)
            .map_err(|_| ())?
            .take(maximum.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|_| ())?;
        (bytes.len() as u64 <= maximum).then_some(bytes).ok_or(())
    }

    fn canonical_regular_file(
        path: &Path,
        parent: &Path,
    ) -> Result<PathBuf, RuntimeUnavailableReason> {
        let canonical =
            fs::canonicalize(path).map_err(|_| RuntimeUnavailableReason::InvalidResource)?;
        if !canonical.is_file() || canonical.parent() != Some(parent) {
            return Err(RuntimeUnavailableReason::InvalidResource);
        }
        Ok(canonical)
    }

    fn verify_binary_identity(
        path: &Path,
        expected_hash: &str,
        expected_version: &str,
        expected_publisher: &str,
    ) -> Result<(), ()> {
        if sha256_file(path)? != expected_hash || !is_x64_pe(path)? {
            return Err(());
        }
        verify_file_version(path, expected_version)?;
        verify_authenticode_publisher(path, expected_publisher)
    }

    fn sha256_file(path: &Path) -> Result<String, ()> {
        let mut input = fs::File::open(path).map_err(|_| ())?;
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = input.read(&mut buffer).map_err(|_| ())?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        Ok(format!("{:x}", hash.finalize()))
    }

    fn is_x64_pe(path: &Path) -> Result<bool, ()> {
        let bytes = bounded_read(path, 128 * 1024 * 1024)?;
        if bytes.get(0..2) != Some(b"MZ") {
            return Ok(false);
        }
        let offset =
            u32::from_le_bytes(bytes.get(0x3c..0x40).ok_or(())?.try_into().unwrap()) as usize;
        if bytes.get(offset..offset + 4) != Some(b"PE\0\0") {
            return Ok(false);
        }
        let machine = u16::from_le_bytes(
            bytes
                .get(offset + 4..offset + 6)
                .ok_or(())?
                .try_into()
                .unwrap(),
        );
        Ok(machine == 0x8664)
    }

    fn verify_file_version(path: &Path, expected: &str) -> Result<(), ()> {
        let path = wide(path.as_os_str());
        let size = unsafe { GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None) };
        if size == 0 {
            return Err(());
        }
        let mut data = vec![0u8; size as usize];
        unsafe { GetFileVersionInfoW(PCWSTR(path.as_ptr()), None, size, data.as_mut_ptr().cast()) }
            .map_err(|_| ())?;
        let root = wide(OsStr::new("\\"));
        let mut info = ptr::null_mut();
        let mut length = 0u32;
        if !unsafe {
            VerQueryValueW(
                data.as_ptr().cast(),
                PCWSTR(root.as_ptr()),
                &mut info,
                &mut length,
            )
        }
        .as_bool()
            || length < size_of::<VS_FIXEDFILEINFO>() as u32
        {
            return Err(());
        }
        let info = unsafe { &*info.cast::<VS_FIXEDFILEINFO>() };
        let actual = format!(
            "{}.{}.{}.{}",
            info.dwFileVersionMS >> 16,
            info.dwFileVersionMS & 0xffff,
            info.dwFileVersionLS >> 16,
            info.dwFileVersionLS & 0xffff
        );
        (actual == expected).then_some(()).ok_or(())
    }

    fn verify_authenticode_publisher(path: &Path, expected: &str) -> Result<(), ()> {
        let path_wide = wide(path.as_os_str());
        let mut file = WINTRUST_FILE_INFO {
            cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
            pcwszFilePath: PCWSTR(path_wide.as_ptr()),
            ..Default::default()
        };
        let mut data = WINTRUST_DATA {
            cbStruct: size_of::<WINTRUST_DATA>() as u32,
            dwUIChoice: WTD_UI_NONE,
            fdwRevocationChecks: WTD_REVOKE_WHOLECHAIN,
            dwUnionChoice: WTD_CHOICE_FILE,
            Anonymous: WINTRUST_DATA_0 {
                pFile: ptr::from_mut(&mut file),
            },
            dwStateAction: WTD_STATEACTION_IGNORE,
            dwProvFlags: WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT,
            ..Default::default()
        };
        let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        if unsafe {
            WinVerifyTrust(
                HWND(-1isize as *mut c_void),
                &mut action,
                ptr::from_mut(&mut data).cast(),
            )
        } != 0
        {
            return Err(());
        }
        signer_subject_matches(&path_wide, expected)
    }

    fn signer_subject_matches(path_wide: &[u16], expected: &str) -> Result<(), ()> {
        let mut store = HCERTSTORE::default();
        let mut message: *mut c_void = ptr::null_mut();
        unsafe {
            CryptQueryObject(
                CERT_QUERY_OBJECT_FILE,
                path_wide.as_ptr().cast(),
                CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
                CERT_QUERY_FORMAT_FLAG_BINARY,
                0,
                None,
                None,
                None,
                Some(&mut store),
                Some(&mut message),
                None,
            )
        }
        .map_err(|_| ())?;
        let result = (|| {
            let mut size = 0u32;
            unsafe { CryptMsgGetParam(message, CMSG_SIGNER_CERT_INFO_PARAM, 0, None, &mut size) }
                .map_err(|_| ())?;
            if size == 0 {
                return Err(());
            }
            let mut signer = vec![0u8; size as usize];
            unsafe {
                CryptMsgGetParam(
                    message,
                    CMSG_SIGNER_CERT_INFO_PARAM,
                    0,
                    Some(signer.as_mut_ptr().cast()),
                    &mut size,
                )
            }
            .map_err(|_| ())?;
            let encoding = X509_ASN_ENCODING | PKCS_7_ASN_ENCODING;
            let certificate = unsafe {
                CertFindCertificateInStore(
                    store,
                    encoding,
                    0,
                    CERT_FIND_SUBJECT_CERT,
                    Some(signer.as_ptr().cast()),
                    None,
                )
            };
            if certificate.is_null() {
                return Err(());
            }
            let comparison = (|| {
                let expected = wide(OsStr::new(expected));
                let mut encoded_size = 0u32;
                let _ = unsafe {
                    CertStrToNameW(
                        encoding,
                        PCWSTR(expected.as_ptr()),
                        CERT_X500_NAME_STR,
                        None,
                        None,
                        &mut encoded_size,
                        None,
                    )
                };
                if encoded_size == 0 {
                    return Err(());
                }
                let mut encoded = vec![0u8; encoded_size as usize];
                unsafe {
                    CertStrToNameW(
                        encoding,
                        PCWSTR(expected.as_ptr()),
                        CERT_X500_NAME_STR,
                        None,
                        Some(encoded.as_mut_ptr()),
                        &mut encoded_size,
                        None,
                    )
                }
                .map_err(|_| ())?;
                let subject = unsafe { &(*(*certificate).pCertInfo).Subject };
                let actual =
                    unsafe { std::slice::from_raw_parts(subject.pbData, subject.cbData as usize) };
                (actual == &encoded[..encoded_size as usize])
                    .then_some(())
                    .ok_or(())
            })();
            let _ = unsafe { CertFreeCertificateContext(Some(certificate)) };
            comparison
        })();
        let _ = unsafe { CryptMsgClose(Some(message)) };
        let _ = unsafe { CertCloseStore(Some(store), 0) };
        result
    }

    fn known_folder(id: &windows::core::GUID) -> Result<PathBuf, ()> {
        let value = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.map_err(|_| ())?;
        let path = unsafe { value.to_string() }.map_err(|_| ())?;
        unsafe { CoTaskMemFree(Some(value.0.cast())) };
        Ok(PathBuf::from(path))
    }

    fn current_user_sid() -> Result<String, ()> {
        let mut token = HANDLE::default();
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
            .map_err(|_| ())?;
        let token = OwnedHandle(token);
        let mut length = 0u32;
        let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut length) };
        if length == 0 {
            return Err(());
        }
        let words = (length as usize).div_ceil(size_of::<usize>());
        let mut storage = vec![0usize; words];
        unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                Some(storage.as_mut_ptr().cast()),
                length,
                &mut length,
            )
        }
        .map_err(|_| ())?;
        let user = unsafe { &*storage.as_ptr().cast::<TOKEN_USER>() };
        let mut sid = PWSTR::null();
        unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid) }.map_err(|_| ())?;
        let value = unsafe { sid.to_string() }.map_err(|_| ())?;
        let _ = unsafe { windows::Win32::Foundation::LocalFree(Some(HLOCAL(sid.0.cast()))) };
        Ok(value)
    }

    fn ensure_current_process_not_elevated() -> Result<(), RuntimeUnavailableReason> {
        let mut token = HANDLE::default();
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
            .map_err(|_| RuntimeUnavailableReason::ProcessLaunchFailed)?;
        let token = OwnedHandle(token);
        let mut elevation = TOKEN_ELEVATION::default();
        let mut length = 0u32;
        unsafe {
            GetTokenInformation(
                token.0,
                TokenElevation,
                Some(ptr::from_mut(&mut elevation).cast()),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut length,
            )
        }
        .map_err(|_| RuntimeUnavailableReason::ProcessLaunchFailed)?;
        (elevation.TokenIsElevated == 0)
            .then_some(())
            .ok_or(RuntimeUnavailableReason::ProcessLaunchFailed)
    }

    fn private_window() -> Result<Option<(HWND, u32)>, RuntimeUnavailableReason> {
        let class = wide(OsStr::new(PRIVATE_WINDOW_CLASS));
        let window = match unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) } {
            Ok(window) => window,
            Err(_) => return Ok(None),
        };
        let mut pid = 0u32;
        if unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) } == 0 || pid == 0 {
            return Err(RuntimeUnavailableReason::ForeignProcess);
        }
        Ok(Some((window, pid)))
    }

    fn process_image_path(handle: HANDLE) -> Result<PathBuf, ()> {
        let mut capacity = 260u32;
        loop {
            let mut buffer = vec![0u16; capacity as usize];
            let mut length = capacity;
            if unsafe {
                QueryFullProcessImageNameW(
                    handle,
                    PROCESS_NAME_WIN32,
                    PWSTR(buffer.as_mut_ptr()),
                    &mut length,
                )
            }
            .is_ok()
            {
                return Ok(PathBuf::from(String::from_utf16_lossy(
                    &buffer[..length as usize],
                )));
            }
            if capacity >= 32_768 {
                return Err(());
            }
            capacity *= 2;
        }
    }

    fn wait_handle(
        handle: HANDLE,
        timeout: Duration,
    ) -> Result<WAIT_EVENT, RuntimeUnavailableReason> {
        let value = unsafe {
            WaitForSingleObject(handle, timeout.as_millis().min(u128::from(u32::MAX)) as u32)
        };
        if value == WAIT_FAILED {
            Err(RuntimeUnavailableReason::ProcessWaitFailed)
        } else {
            Ok(value)
        }
    }

    fn query_private_service(manifest: &RuntimeManifest) -> Result<bool, RuntimeUnavailableReason> {
        let manager = unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) }
            .map_err(|_| RuntimeUnavailableReason::ServiceUnavailable)?;
        let name = wide(OsStr::new(SERVICE_NAME));
        let service = unsafe {
            OpenServiceW(
                manager,
                PCWSTR(name.as_ptr()),
                SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS,
            )
        };
        let service = match service {
            Ok(service) => service,
            Err(_) => {
                let _ = unsafe { CloseServiceHandle(manager) };
                return Err(RuntimeUnavailableReason::ServiceUnavailable);
            }
        };
        let result = (|| {
            let mut needed = 0u32;
            let _ = unsafe { QueryServiceConfigW(service, None, 0, &mut needed) };
            if needed == 0 {
                return Err(RuntimeUnavailableReason::ServiceUnavailable);
            }
            let words = (needed as usize).div_ceil(size_of::<usize>());
            let mut storage = vec![0usize; words];
            unsafe {
                QueryServiceConfigW(
                    service,
                    Some(storage.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>()),
                    needed,
                    &mut needed,
                )
            }
            .map_err(|_| RuntimeUnavailableReason::ServiceUnavailable)?;
            let config = unsafe { &*storage.as_ptr().cast::<QUERY_SERVICE_CONFIGW>() };
            let command = unsafe { config.lpBinaryPathName.to_string() }
                .map_err(|_| RuntimeUnavailableReason::ServiceUnavailable)?;
            if !service_command_matches(&command, manifest) {
                return Err(RuntimeUnavailableReason::ServiceUnavailable);
            }
            let mut status = SERVICE_STATUS_PROCESS::default();
            let status_bytes = unsafe {
                std::slice::from_raw_parts_mut(
                    ptr::from_mut(&mut status).cast::<u8>(),
                    size_of::<SERVICE_STATUS_PROCESS>(),
                )
            };
            unsafe {
                QueryServiceStatusEx(
                    service,
                    SC_STATUS_PROCESS_INFO,
                    Some(status_bytes),
                    &mut needed,
                )
            }
            .map_err(|_| RuntimeUnavailableReason::ServiceUnavailable)?;
            Ok(status.dwCurrentState == SERVICE_RUNNING)
        })();
        let _ = unsafe { CloseServiceHandle(service) };
        let _ = unsafe { CloseServiceHandle(manager) };
        result
    }

    pub(super) fn service_command_matches(command: &str, manifest: &RuntimeManifest) -> bool {
        let Some(rest) = command.strip_prefix('"') else {
            return false;
        };
        let Some(end) = rest.find('"') else {
            return false;
        };
        let executable = &rest[..end];
        let Some(arguments) = rest.get(end + 1..) else {
            return false;
        };
        Path::new(executable) == manifest.service_binary_path
            && service_arguments_match(arguments.trim(), manifest)
    }

    fn service_arguments_match(arguments: &str, manifest: &RuntimeManifest) -> bool {
        let Some(tokens) = split_windows_arguments(arguments) else {
            return false;
        };
        let mut service = false;
        let mut instance = None;
        let mut pipe = None;
        let mut sddl = None;
        let mut index = 0usize;
        while index < tokens.len() {
            match tokens[index].as_str() {
                "-svc" if !service => service = true,
                "-instance" | "-service-pipe-name" | "-service-security-descriptor"
                    if index + 1 < tokens.len() =>
                {
                    let slot = match tokens[index].as_str() {
                        "-instance" => &mut instance,
                        "-service-pipe-name" => &mut pipe,
                        _ => &mut sddl,
                    };
                    if slot.replace(tokens[index + 1].clone()).is_some() {
                        return false;
                    }
                    index += 1;
                }
                _ => return false,
            }
            index += 1;
        }
        service
            && instance.as_deref() == Some(INSTANCE_NAME)
            && pipe.as_deref() == Some(PIPE_NAME)
            && sddl.as_deref() == Some(manifest.expanded_sddl.as_str())
    }

    fn split_windows_arguments(value: &str) -> Option<Vec<String>> {
        let mut result = Vec::new();
        let mut current = String::new();
        let mut quoted = false;
        for character in value.chars() {
            match character {
                '"' => quoted = !quoted,
                ' ' | '\t' if !quoted && !current.is_empty() => {
                    result.push(std::mem::take(&mut current));
                }
                ' ' | '\t' if !quoted => {}
                _ => current.push(character),
            }
        }
        if quoted {
            return None;
        }
        if !current.is_empty() {
            result.push(current);
        }
        Some(result)
    }

    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(Some(0)).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use everything_ipc::client::EverythingClientError;
    use everything_ipc::protocol::{EverythingQueryResult, EverythingQuerySpec, EverythingSort};

    use super::{
        build_runtime_launch, managed_ini_bytes, parse_reviewed_lock, parse_runtime_manifest,
        start_runtime_with, EverythingRuntimeClient, EverythingRuntimeStatus,
        EverythingRuntimeSupervisor, OwnedRuntimeProcess, ReadyClientProvider, RuntimeBackend,
        RuntimeLaunch, RuntimeResourcePaths, RuntimeSystem, RuntimeUnavailableReason,
        StartedRuntime, ValidatedManifest, VerifiedResources, INSTANCE_NAME, PIPE_NAME,
        REVIEWED_LOCK_BYTES, SERVICE_NAME,
    };

    const WAIT_LIMIT: Duration = Duration::from_secs(2);

    struct FakeClient {
        loaded: bool,
        result: EverythingQueryResult,
    }

    impl FakeClient {
        fn ready(total: u32) -> Arc<Self> {
            Arc::new(Self {
                loaded: true,
                result: EverythingQueryResult {
                    total,
                    request_flags: 0x155,
                    sort_type: 14,
                    items: Vec::new(),
                },
            })
        }
    }

    impl EverythingRuntimeClient for FakeClient {
        fn database_loaded(&self, _timeout: Duration) -> Result<bool, EverythingClientError> {
            Ok(self.loaded)
        }

        fn query(
            &self,
            spec: EverythingQuerySpec,
        ) -> Result<EverythingQueryResult, EverythingClientError> {
            assert!(spec.search.is_empty());
            assert_eq!(spec.offset, 0);
            assert_eq!(spec.max_results, 1);
            assert_eq!(spec.request_flags, 0x155);
            assert_eq!(spec.sort, EverythingSort::DateModifiedDescending);
            Ok(self.result.clone())
        }
    }

    #[derive(Default)]
    struct ProcessControl {
        exited: AtomicBool,
        shutdowns: AtomicUsize,
    }

    impl ProcessControl {
        fn exit(&self) {
            self.exited.store(true, Ordering::Release);
        }
    }

    struct FakeProcess(Arc<ProcessControl>);

    impl OwnedRuntimeProcess for FakeProcess {
        fn poll_exited(&mut self) -> Result<bool, RuntimeUnavailableReason> {
            Ok(self.0.exited.load(Ordering::Acquire))
        }

        fn shutdown(&mut self) {
            self.0.shutdowns.fetch_add(1, Ordering::AcqRel);
        }
    }

    enum StartPlan {
        Ready {
            client: Arc<dyn EverythingRuntimeClient>,
            process: Arc<ProcessControl>,
        },
        Fail(RuntimeUnavailableReason),
    }

    struct FakeBackend {
        starts: AtomicUsize,
        plans: Mutex<VecDeque<StartPlan>>,
        release_first: AtomicBool,
        block_first: bool,
    }

    impl FakeBackend {
        fn new(plans: Vec<StartPlan>) -> Arc<Self> {
            Arc::new(Self {
                starts: AtomicUsize::new(0),
                plans: Mutex::new(plans.into()),
                release_first: AtomicBool::new(true),
                block_first: false,
            })
        }

        fn blocked(plans: Vec<StartPlan>) -> Arc<Self> {
            Arc::new(Self {
                starts: AtomicUsize::new(0),
                plans: Mutex::new(plans.into()),
                release_first: AtomicBool::new(false),
                block_first: true,
            })
        }
    }

    impl RuntimeBackend for FakeBackend {
        fn start(
            &self,
            deadline: Instant,
            cancelled: &(dyn Fn() -> bool + Send + Sync),
        ) -> Result<StartedRuntime, RuntimeUnavailableReason> {
            let attempt = self.starts.fetch_add(1, Ordering::AcqRel);
            while self.block_first && attempt == 0 && !self.release_first.load(Ordering::Acquire) {
                if cancelled() || Instant::now() >= deadline {
                    return Err(RuntimeUnavailableReason::StartupTimedOut);
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            match self
                .plans
                .lock()
                .unwrap()
                .pop_front()
                .expect("missing start plan")
            {
                StartPlan::Ready { client, process } => {
                    Ok(StartedRuntime::new(client, Box::new(FakeProcess(process))))
                }
                StartPlan::Fail(reason) => Err(reason),
            }
        }
    }

    fn ready_plan(total: u32) -> (StartPlan, Arc<ProcessControl>) {
        let process = Arc::new(ProcessControl::default());
        (
            StartPlan::Ready {
                client: FakeClient::ready(total),
                process: Arc::clone(&process),
            },
            process,
        )
    }

    fn supervisor(backend: Arc<dyn RuntimeBackend>) -> EverythingRuntimeSupervisor {
        EverythingRuntimeSupervisor::with_backend(
            backend,
            Duration::from_secs(1),
            Duration::from_millis(1),
            [Duration::ZERO; 3],
        )
    }

    fn wait_until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + WAIT_LIMIT;
        while !condition() {
            assert!(Instant::now() < deadline, "condition timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn startup_returns_immediately_and_an_empty_loaded_index_becomes_ready() {
        let (plan, _process) = ready_plan(0);
        let backend = FakeBackend::blocked(vec![plan]);
        let runtime = supervisor(Arc::clone(&backend) as Arc<dyn RuntimeBackend>);

        let started = Instant::now();
        runtime.start_async();
        assert!(started.elapsed() < Duration::from_millis(50));
        assert_eq!(runtime.snapshot().status, EverythingRuntimeStatus::Starting);
        assert!(matches!(
            runtime.ready_client(),
            Err(EverythingRuntimeStatus::Starting)
        ));

        backend.release_first.store(true, Ordering::Release);
        wait_until(|| runtime.snapshot().status == EverythingRuntimeStatus::Ready);
        let lease = runtime.ready_client().expect("ready lease missing");
        assert_eq!(lease.generation(), 1);
        runtime.shutdown();
    }

    #[test]
    fn fixed_query2_probe_mismatch_is_terminal_unavailable() {
        let process = Arc::new(ProcessControl::default());
        let client: Arc<dyn EverythingRuntimeClient> = Arc::new(FakeClient {
            loaded: true,
            result: EverythingQueryResult {
                total: 0,
                request_flags: 0x154,
                sort_type: 14,
                items: Vec::new(),
            },
        });
        let backend = FakeBackend::new(vec![StartPlan::Ready { client, process }]);
        let runtime = supervisor(Arc::clone(&backend) as Arc<dyn RuntimeBackend>);

        runtime.start_async();
        wait_until(|| runtime.snapshot().status == EverythingRuntimeStatus::Unavailable);
        assert_eq!(
            runtime.snapshot().reason,
            Some(RuntimeUnavailableReason::CapabilityMismatch)
        );
        assert_eq!(backend.starts.load(Ordering::Acquire), 1);
        runtime.shutdown();
    }

    #[test]
    fn identity_and_single_owner_failures_are_terminal_without_retry() {
        for reason in [
            RuntimeUnavailableReason::RuntimeAlreadyOwned,
            RuntimeUnavailableReason::InvalidResource,
            RuntimeUnavailableReason::InvalidManifest,
            RuntimeUnavailableReason::ForeignProcess,
            RuntimeUnavailableReason::ServiceUnavailable,
        ] {
            let backend = FakeBackend::new(vec![StartPlan::Fail(reason)]);
            let runtime = supervisor(Arc::clone(&backend) as Arc<dyn RuntimeBackend>);
            runtime.start_async();
            wait_until(|| runtime.snapshot().status == EverythingRuntimeStatus::Unavailable);
            assert_eq!(runtime.snapshot().reason, Some(reason));
            assert_eq!(backend.starts.load(Ordering::Acquire), 1);
            runtime.shutdown();
        }
    }

    #[test]
    fn unexpected_exit_restarts_three_times_then_becomes_terminal() {
        let mut plans = Vec::new();
        let mut processes = Vec::new();
        for _ in 0..4 {
            let (plan, process) = ready_plan(1);
            plans.push(plan);
            processes.push(process);
        }
        let backend = FakeBackend::new(plans);
        let runtime = supervisor(Arc::clone(&backend) as Arc<dyn RuntimeBackend>);
        runtime.start_async();

        for (index, process) in processes.iter().enumerate() {
            wait_until(|| {
                runtime.snapshot().status == EverythingRuntimeStatus::Ready
                    && runtime.ready_client().map(|lease| lease.generation()).ok()
                        == Some((index + 1) as u64)
            });
            process.exit();
        }
        wait_until(|| runtime.snapshot().status == EverythingRuntimeStatus::Unavailable);
        assert_eq!(
            runtime.snapshot().reason,
            Some(RuntimeUnavailableReason::RetriesExhausted)
        );
        assert_eq!(backend.starts.load(Ordering::Acquire), 4);
        runtime.shutdown();
    }

    #[test]
    fn stale_generation_failure_cannot_evict_the_published_replacement() {
        let (first_plan, first_process) = ready_plan(1);
        let (second_plan, _second_process) = ready_plan(1);
        let (third_plan, _third_process) = ready_plan(1);
        let backend = FakeBackend::new(vec![first_plan, second_plan, third_plan]);
        let runtime = supervisor(backend as Arc<dyn RuntimeBackend>);
        runtime.start_async();
        wait_until(|| runtime.snapshot().status == EverythingRuntimeStatus::Ready);
        let first = runtime.ready_client().unwrap();

        first_process.exit();
        wait_until(|| runtime.ready_client().map(|lease| lease.generation()).ok() == Some(2));
        runtime.invalidate_if_current(first.generation());
        assert_eq!(runtime.ready_client().unwrap().generation(), 2);

        runtime.invalidate_if_current(2);
        assert!(matches!(
            runtime.ready_client(),
            Err(EverythingRuntimeStatus::Starting)
        ));
        wait_until(|| runtime.ready_client().map(|lease| lease.generation()).ok() == Some(3));
        runtime.shutdown();
    }

    #[test]
    fn shutdown_is_idempotent_and_rejects_queries_before_releasing_process() {
        let (plan, process) = ready_plan(1);
        let runtime = supervisor(FakeBackend::new(vec![plan]) as Arc<dyn RuntimeBackend>);
        runtime.start_async();
        wait_until(|| runtime.snapshot().status == EverythingRuntimeStatus::Ready);

        runtime.shutdown();
        assert_eq!(runtime.snapshot().status, EverythingRuntimeStatus::Stopping);
        assert!(matches!(
            runtime.ready_client(),
            Err(EverythingRuntimeStatus::Stopping)
        ));
        runtime.shutdown();
        assert_eq!(process.shutdowns.load(Ordering::Acquire), 1);
    }

    struct PipelineGuard(Arc<Mutex<Vec<String>>>);

    impl Drop for PipelineGuard {
        fn drop(&mut self) {
            self.0.lock().unwrap().push("release-mutex".into());
        }
    }

    struct PipelineSystem {
        events: Arc<Mutex<Vec<String>>>,
        failure: Option<RuntimeUnavailableReason>,
        orphan_pid: Option<u32>,
        orphan_managed: bool,
        service_running: bool,
        connect_failures: AtomicUsize,
        process: Arc<ProcessControl>,
    }

    impl PipelineSystem {
        fn new() -> Self {
            Self {
                events: Arc::new(Mutex::new(Vec::new())),
                failure: None,
                orphan_pid: None,
                orphan_managed: true,
                service_running: true,
                connect_failures: AtomicUsize::new(0),
                process: Arc::new(ProcessControl::default()),
            }
        }

        fn event(&self, event: impl Into<String>) {
            self.events.lock().unwrap().push(event.into());
        }
    }

    impl RuntimeSystem for PipelineSystem {
        type Guard = PipelineGuard;
        type Process = FakeProcess;

        fn verify_resources(&self) -> Result<VerifiedResources, RuntimeUnavailableReason> {
            self.event("verify-resources");
            if self.failure == Some(RuntimeUnavailableReason::InvalidResource) {
                Err(RuntimeUnavailableReason::InvalidResource)
            } else {
                Ok(VerifiedResources::for_test())
            }
        }

        fn verify_manifest(
            &self,
            _resources: &VerifiedResources,
        ) -> Result<ValidatedManifest, RuntimeUnavailableReason> {
            self.event("verify-manifest");
            if self.failure == Some(RuntimeUnavailableReason::InvalidManifest) {
                Err(RuntimeUnavailableReason::InvalidManifest)
            } else {
                Ok(ValidatedManifest::for_test())
            }
        }

        fn acquire_runtime_mutex(&self) -> Result<Self::Guard, RuntimeUnavailableReason> {
            self.event("acquire-mutex");
            if self.failure == Some(RuntimeUnavailableReason::RuntimeAlreadyOwned) {
                Err(RuntimeUnavailableReason::RuntimeAlreadyOwned)
            } else {
                Ok(PipelineGuard(Arc::clone(&self.events)))
            }
        }

        fn private_window_pid(&self) -> Result<Option<u32>, RuntimeUnavailableReason> {
            self.event("inspect-window");
            Ok(self.orphan_pid)
        }

        fn orphan_is_managed(
            &self,
            pid: u32,
            _resources: &VerifiedResources,
        ) -> Result<bool, RuntimeUnavailableReason> {
            self.event(format!("authenticate-orphan:{pid}"));
            Ok(self.orphan_managed)
        }

        fn stop_orphan(
            &self,
            pid: u32,
            _deadline: Instant,
        ) -> Result<(), RuntimeUnavailableReason> {
            self.event(format!("stop-orphan:{pid}"));
            Ok(())
        }

        fn prepare_launch(
            &self,
            _resources: &VerifiedResources,
        ) -> Result<RuntimeLaunch, RuntimeUnavailableReason> {
            self.event("prepare-launch");
            Ok(RuntimeLaunch::for_test())
        }

        fn service_is_running(
            &self,
            _manifest: &ValidatedManifest,
        ) -> Result<bool, RuntimeUnavailableReason> {
            self.event("service-running");
            Ok(self.service_running)
        }

        fn launch_process(
            &self,
            _launch: RuntimeLaunch,
        ) -> Result<Self::Process, RuntimeUnavailableReason> {
            self.event("launch-process");
            Ok(FakeProcess(Arc::clone(&self.process)))
        }

        fn connect_owned(
            &self,
            pid: u32,
            _timeout: Duration,
        ) -> Result<Arc<dyn EverythingRuntimeClient>, RuntimeUnavailableReason> {
            self.event(format!("connect-owned:{pid}"));
            if self
                .connect_failures
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                return Err(RuntimeUnavailableReason::IpcUnavailable);
            }
            Ok(FakeClient::ready(0))
        }

        fn process_pid(&self, _process: &Self::Process) -> u32 {
            77
        }
    }

    #[test]
    fn startup_pipeline_authenticates_and_stops_only_the_exact_managed_orphan() {
        let mut system = PipelineSystem::new();
        system.orphan_pid = Some(41);
        let events = Arc::clone(&system.events);
        let mut runtime =
            start_runtime_with(&system, Instant::now() + Duration::from_secs(1), &|| false)
                .unwrap();

        assert_eq!(
            events.lock().unwrap().as_slice(),
            [
                "verify-resources",
                "verify-manifest",
                "acquire-mutex",
                "inspect-window",
                "authenticate-orphan:41",
                "stop-orphan:41",
                "prepare-launch",
                "service-running",
                "launch-process",
                "connect-owned:77",
            ]
        );
        runtime.process.shutdown();
        drop(runtime);
        assert_eq!(events.lock().unwrap().last().unwrap(), "release-mutex");
    }

    #[test]
    fn startup_pipeline_fails_closed_before_unsafe_follow_on_work() {
        for (failure, expected_events) in [
            (
                RuntimeUnavailableReason::InvalidResource,
                vec!["verify-resources"],
            ),
            (
                RuntimeUnavailableReason::InvalidManifest,
                vec!["verify-resources", "verify-manifest"],
            ),
            (
                RuntimeUnavailableReason::RuntimeAlreadyOwned,
                vec!["verify-resources", "verify-manifest", "acquire-mutex"],
            ),
        ] {
            let mut system = PipelineSystem::new();
            system.failure = Some(failure);
            let result =
                start_runtime_with(&system, Instant::now() + Duration::from_secs(1), &|| false);
            assert!(matches!(result, Err(reason) if reason == failure));
            assert_eq!(
                system.events.lock().unwrap().as_slice(),
                expected_events,
                "{failure:?}"
            );
        }

        let mut foreign = PipelineSystem::new();
        foreign.orphan_pid = Some(42);
        foreign.orphan_managed = false;
        assert!(matches!(
            start_runtime_with(&foreign, Instant::now() + Duration::from_secs(1), &|| false,),
            Err(RuntimeUnavailableReason::ForeignProcess)
        ));
        assert!(!foreign
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|event| event.starts_with("stop-orphan")));

        let mut stopped = PipelineSystem::new();
        stopped.service_running = false;
        assert!(matches!(
            start_runtime_with(&stopped, Instant::now() + Duration::from_secs(1), &|| false,),
            Err(RuntimeUnavailableReason::ServiceUnavailable)
        ));
        assert!(!stopped
            .events
            .lock()
            .unwrap()
            .contains(&"launch-process".into()));
    }

    #[test]
    fn startup_pipeline_retries_only_transient_ipc_connection_failures() {
        let system = PipelineSystem::new();
        system.connect_failures.store(2, Ordering::Release);
        let mut runtime =
            start_runtime_with(&system, Instant::now() + Duration::from_secs(1), &|| false)
                .unwrap();

        assert_eq!(
            system
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| event.as_str() == "connect-owned:77")
                .count(),
            3
        );
        runtime.process.shutdown();
    }

    #[test]
    fn resource_lock_manifest_and_fixed_runtime_identity_are_strict() {
        let lock = parse_reviewed_lock(include_bytes!(
            "../../resources/everything/everything.lock.json"
        ))
        .unwrap();
        assert_eq!(lock.version, "1.4.1.1032");
        assert_eq!(lock.architecture, "x64");

        let manifest = format!(
            r#"{{"schema_version":1,"version":"{}","architecture":"x64","sha256":"{}","publisher":{},"instance":"{}","service_name":"{}","pipe_name":{},"desktop_user_sid":"S-1-5-21-1-2-3-1001","expanded_sddl":"D:P(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)(A;OICI;GRGW;;;S-1-5-21-1-2-3-1001)","service_binary_path":"C:\\ProgramData\\UiPilot_tools\\everything\\{}\\Everything.exe"}}"#,
            lock.version,
            lock.everything_exe_sha256,
            serde_json::to_string(&lock.authenticode_publisher).unwrap(),
            INSTANCE_NAME,
            SERVICE_NAME,
            serde_json::to_string(PIPE_NAME).unwrap(),
            lock.version,
        );
        assert!(parse_runtime_manifest(manifest.as_bytes(), &lock).is_ok());
        assert!(parse_runtime_manifest(
            manifest
                .replace("\"instance\":\"UiPilot\"", "\"instance\":\"Everything\"")
                .as_bytes(),
            &lock,
        )
        .is_err());
        assert!(parse_runtime_manifest(
            manifest
                .replace(
                    "\"schema_version\":1",
                    "\"schema_version\":1,\"extra\":true"
                )
                .as_bytes(),
            &lock,
        )
        .is_err());
        let noncanonical_sid = manifest.replace("S-1-5-21-1-2-3-1001", "S-1-05-21-1-2-3-1001");
        assert!(parse_runtime_manifest(noncanonical_sid.as_bytes(), &lock).is_err());

        let noncanonical_lock = String::from_utf8(REVIEWED_LOCK_BYTES.to_vec())
            .unwrap()
            .replace("1.4.1.1032", "01.4.1.1032");
        assert!(parse_reviewed_lock(noncanonical_lock.as_bytes()).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn service_contract_accepts_argument_order_but_rejects_duplicates() {
        let manifest = ValidatedManifest::for_test().manifest;
        let executable = manifest.service_binary_path.display();
        let reordered = format!(
            r#""{executable}" -service-security-descriptor "{}" -service-pipe-name "{}" -svc -instance "{}""#,
            manifest.expanded_sddl, PIPE_NAME, INSTANCE_NAME,
        );
        assert!(super::windows_runtime::service_command_matches(
            &reordered, &manifest,
        ));
        assert!(!super::windows_runtime::service_command_matches(
            &format!("{reordered} -svc"),
            &manifest,
        ));
    }

    #[test]
    fn resource_paths_ini_and_launch_arguments_are_fixed_and_private() {
        let root = std::path::Path::new(r"C:\Program Files\UiPilot_tools");
        let paths = RuntimeResourcePaths::resolve(root);
        assert_eq!(
            paths.executable,
            root.join(r"resources\everything\Everything.exe")
        );
        assert_eq!(
            paths.lock,
            root.join(r"resources\everything\everything.lock.json")
        );
        assert_eq!(
            paths.template,
            root.join(r"resources\everything\Everything-UiPilot.ini")
        );

        let template = include_bytes!("../../resources/everything/Everything-UiPilot.ini");
        let regenerated = managed_ini_bytes(template).unwrap();
        assert_eq!(regenerated, template);
        let tampered = String::from_utf8(template.to_vec())
            .unwrap()
            .replace("show_tray_icon=0", "show_tray_icon=1");
        assert!(managed_ini_bytes(tampered.as_bytes()).is_err());

        let launch = build_runtime_launch(
            paths.executable.clone(),
            std::path::PathBuf::from(
                r"C:\Users\me\AppData\Local\UiPilot_tools\everything\7\Everything-UiPilot.ini",
            ),
            std::path::PathBuf::from(
                r"C:\Users\me\AppData\Local\UiPilot_tools\everything\7\Everything-UiPilot.db",
            ),
        )
        .unwrap();
        let arguments = launch
            .command
            .arguments()
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            arguments,
            [
                "-instance",
                INSTANCE_NAME,
                "-service-pipe-name",
                PIPE_NAME,
                "-config",
                r"C:\Users\me\AppData\Local\UiPilot_tools\everything\7\Everything-UiPilot.ini",
                "-db",
                r"C:\Users\me\AppData\Local\UiPilot_tools\everything\7\Everything-UiPilot.db",
                "-startup",
            ]
        );
    }
}
