use std::{
    collections::BTreeSet,
    error::Error,
    ffi::OsStr,
    fmt,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct HostInstanceId([u8; 16]);

impl HostInstanceId {
    pub(super) fn new(value: [u8; 16]) -> Result<Self, ProcessAuthorityInitError> {
        if value == [0; 16] {
            return Err(ProcessAuthorityInitError::Unavailable);
        }
        Ok(Self(value))
    }

    pub(crate) fn as_bytes(self) -> [u8; 16] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProcessAuthorityInitError {
    Unavailable,
}

impl fmt::Display for ProcessAuthorityInitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Unavailable")
    }
}

impl Error for ProcessAuthorityInitError {}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct ProcessTarget {
    host_instance_id: HostInstanceId,
    pid: u32,
    creation_time: u64,
}

impl ProcessTarget {
    pub(super) fn new(
        host_instance_id: HostInstanceId,
        pid: u32,
        creation_time: u64,
    ) -> Result<Self, ProcessError> {
        if pid == 0 || creation_time == 0 {
            return Err(ProcessError::InvalidTarget);
        }
        Ok(Self {
            host_instance_id,
            pid,
            creation_time,
        })
    }

    #[cfg(test)]
    pub(super) fn from_parts_for_test(
        host_instance_id: [u8; 16],
        pid: u32,
        creation_time: u64,
    ) -> Self {
        Self {
            host_instance_id: HostInstanceId(host_instance_id),
            pid,
            creation_time,
        }
    }

    fn is_structurally_valid(&self) -> bool {
        self.host_instance_id.0 != [0; 16] && self.pid != 0 && self.creation_time != 0
    }

    pub(crate) fn host_instance_id(&self) -> HostInstanceId {
        self.host_instance_id
    }

    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    pub(crate) fn creation_time(&self) -> u64 {
        self.creation_time
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProcessRecord {
    target: ProcessTarget,
    pid: u32,
    name: String,
    executable_path: Option<PathBuf>,
    observed_parent_pid: Option<u32>,
}

impl ProcessRecord {
    fn new(
        target: ProcessTarget,
        name: String,
        executable_path: Option<PathBuf>,
        observed_parent_pid: Option<u32>,
    ) -> Result<Self, ProcessError> {
        if !valid_name(&name)
            || executable_path
                .as_deref()
                .is_some_and(|path| !valid_path(path))
        {
            return Err(ProcessError::InvalidTarget);
        }
        Ok(Self {
            pid: target.pid,
            target,
            name,
            executable_path,
            observed_parent_pid: observed_parent_pid.filter(|pid| *pid != 0),
        })
    }

    pub(crate) fn target(&self) -> &ProcessTarget {
        &self.target
    }

    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn executable_path(&self) -> Option<&Path> {
        self.executable_path.as_deref()
    }

    pub(crate) fn observed_parent_pid(&self) -> Option<u32> {
        self.observed_parent_pid
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ProcessOmissions {
    exited_or_changed: u64,
    inspection_denied: u64,
    identity_unavailable: u64,
}

impl ProcessOmissions {
    pub(crate) fn exited_or_changed(&self) -> u64 {
        self.exited_or_changed
    }

    pub(crate) fn inspection_denied(&self) -> u64 {
        self.inspection_denied
    }

    pub(crate) fn identity_unavailable(&self) -> u64 {
        self.identity_unavailable
    }

    pub(crate) fn total(&self) -> u64 {
        self.exited_or_changed
            .saturating_add(self.inspection_denied)
            .saturating_add(self.identity_unavailable)
    }

    fn increment(&mut self, kind: OmissionKind) -> Result<(), ProcessError> {
        let counter = match kind {
            OmissionKind::ExitedOrChanged => &mut self.exited_or_changed,
            OmissionKind::InspectionDenied => &mut self.inspection_denied,
            OmissionKind::IdentityUnavailable => &mut self.identity_unavailable,
        };
        *counter = counter
            .checked_add(1)
            .ok_or(ProcessError::CatalogUnavailable)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProcessSnapshot {
    processes: Vec<ProcessRecord>,
    omissions: ProcessOmissions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProcessIntent {
    Terminate,
    ForceTerminate,
    Restart,
}

impl ProcessSnapshot {
    pub(crate) fn processes(&self) -> &[ProcessRecord] {
        &self.processes
    }

    pub(crate) fn omissions(&self) -> &ProcessOmissions {
        &self.omissions
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProcessError {
    CatalogUnavailable,
    InvalidTarget,
    ExpiredTarget,
    TargetExited,
    TargetChanged,
    InspectionDenied,
    ProtectedTarget,
    PlatformUnsupported,
}

impl fmt::Display for ProcessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CatalogUnavailable => "CatalogUnavailable",
            Self::InvalidTarget => "InvalidTarget",
            Self::ExpiredTarget => "ExpiredTarget",
            Self::TargetExited => "TargetExited",
            Self::TargetChanged => "TargetChanged",
            Self::InspectionDenied => "InspectionDenied",
            Self::ProtectedTarget => "ProtectedTarget",
            Self::PlatformUnsupported => "PlatformUnsupported",
        })
    }
}

impl Error for ProcessError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ObservationAccess {
    query_limited_information: bool,
    synchronize: bool,
}

impl ObservationAccess {
    pub(super) const SNAPSHOT: Self = Self {
        query_limited_information: true,
        synchronize: true,
    };

    pub(super) fn query_limited_information(self) -> bool {
        self.query_limited_information
    }

    pub(super) fn synchronize(self) -> bool {
        self.synchronize
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ProcessCandidate {
    pid: u32,
    name: String,
    observed_parent_pid: Option<u32>,
}

impl ProcessCandidate {
    pub(super) fn new(pid: u32, name: String, observed_parent_pid: Option<u32>) -> Self {
        Self {
            pid,
            name,
            observed_parent_pid,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ProcessObservation {
    name: String,
    executable_path: PathBuf,
}

impl ProcessObservation {
    pub(super) fn new(name: String, executable_path: PathBuf) -> Self {
        Self {
            name,
            executable_path,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ProcessProtection {
    critical: bool,
    protected: bool,
}

impl ProcessProtection {
    pub(super) fn new(critical: bool, protected: bool) -> Self {
        Self {
            critical,
            protected,
        }
    }

    pub(super) fn is_critical(self) -> bool {
        self.critical
    }

    pub(super) fn is_protected(self) -> bool {
        self.protected
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Liveness {
    Alive,
    Exited,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ObservationFailure {
    ExitedOrChanged,
    AccessDenied,
    IdentityUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum EnumerationFailure {
    Unavailable,
}

pub(super) trait ProcessPlatformAdapter {
    /// The associated type owns the platform resource and releases it from `Drop`.
    type Handle;

    fn enumerate_candidates(
        &self,
    ) -> Result<Vec<Result<ProcessCandidate, EnumerationFailure>>, EnumerationFailure>;

    fn open_observation(
        &self,
        pid: u32,
        access: ObservationAccess,
    ) -> Result<Self::Handle, ObservationFailure>;

    fn creation_time(&self, handle: &Self::Handle) -> Result<u64, ObservationFailure>;

    fn liveness(&self, handle: &Self::Handle) -> Result<Liveness, ObservationFailure>;

    fn observation(&self, handle: &Self::Handle) -> Result<ProcessObservation, ObservationFailure>;

    fn reenumerate_name(&self, pid: u32) -> Result<Option<String>, ObservationFailure>;

    fn process_names_equivalent(
        &self,
        left: &OsStr,
        right: &OsStr,
    ) -> Result<bool, ObservationFailure>;

    fn protection(&self, handle: &Self::Handle) -> Result<ProcessProtection, ObservationFailure>;
}

pub(super) struct CoreAuthority<A: ProcessPlatformAdapter> {
    host_instance_id: HostInstanceId,
    self_pid: u32,
    self_creation_time: u64,
    adapter: A,
}

impl<A: ProcessPlatformAdapter> CoreAuthority<A> {
    pub(super) fn new(
        host_instance_id: HostInstanceId,
        self_target: ProcessTarget,
        adapter: A,
    ) -> Self {
        Self {
            host_instance_id,
            self_pid: self_target.pid,
            self_creation_time: self_target.creation_time,
            adapter,
        }
    }

    pub(super) fn snapshot(&self) -> Result<ProcessSnapshot, ProcessError> {
        let enumerated = self
            .adapter
            .enumerate_candidates()
            .map_err(|_| ProcessError::CatalogUnavailable)?;
        let candidates = enumerated
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ProcessError::CatalogUnavailable)?;
        let mut seen_pids = BTreeSet::new();
        let mut processes = Vec::new();
        let mut omissions = ProcessOmissions::default();

        for candidate in candidates {
            if candidate.pid == 0 {
                omissions.increment(OmissionKind::IdentityUnavailable)?;
                continue;
            }
            if !seen_pids.insert(candidate.pid) {
                omissions.increment(OmissionKind::ExitedOrChanged)?;
                continue;
            }
            if !valid_name(&candidate.name) {
                omissions.increment(OmissionKind::IdentityUnavailable)?;
                continue;
            }
            match self.snapshot_candidate(candidate) {
                Ok(record) => processes.push(record),
                Err(kind) => omissions.increment(kind)?,
            }
        }

        processes.sort_by(|left, right| {
            left.pid
                .cmp(&right.pid)
                .then_with(|| left.target.creation_time.cmp(&right.target.creation_time))
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.executable_path.cmp(&right.executable_path))
                .then_with(|| left.observed_parent_pid.cmp(&right.observed_parent_pid))
        });
        Ok(ProcessSnapshot {
            processes,
            omissions,
        })
    }

    pub(super) fn admit(
        &self,
        target: &ProcessTarget,
        intent: ProcessIntent,
    ) -> Result<CoreAdmission<A::Handle>, ProcessError> {
        if !target.is_structurally_valid() {
            return Err(ProcessError::InvalidTarget);
        }
        if target.host_instance_id != self.host_instance_id {
            return Err(ProcessError::ExpiredTarget);
        }

        let handle = self
            .adapter
            .open_observation(target.pid, ObservationAccess::SNAPSHOT)
            .map_err(admission_identity_failure)?;
        let creation_time = self
            .adapter
            .creation_time(&handle)
            .map_err(admission_identity_failure)?;
        if creation_time == 0 {
            return Err(ProcessError::InspectionDenied);
        }
        if creation_time != target.creation_time {
            return Err(ProcessError::TargetChanged);
        }

        match self.adapter.liveness(&handle) {
            Ok(Liveness::Alive) => {}
            Ok(Liveness::Exited) => return Err(ProcessError::TargetExited),
            Err(_) => return Err(ProcessError::InspectionDenied),
        }

        let protection = self
            .adapter
            .protection(&handle)
            .map_err(|_| ProcessError::InspectionDenied)?;
        if target.pid == self.self_pid && target.creation_time == self.self_creation_time {
            return Err(ProcessError::ProtectedTarget);
        }
        if protection.is_critical() || protection.is_protected() {
            return Err(ProcessError::ProtectedTarget);
        }

        Ok(CoreAdmission {
            target: *target,
            intent,
            _handle: handle,
        })
    }

    fn snapshot_candidate(
        &self,
        candidate: ProcessCandidate,
    ) -> Result<ProcessRecord, OmissionKind> {
        let handle = self
            .adapter
            .open_observation(candidate.pid, ObservationAccess::SNAPSHOT)
            .map_err(OmissionKind::from)?;
        let first_creation = self.adapter.creation_time(&handle);
        let first_liveness = self.adapter.liveness(&handle);
        let observation = self.adapter.observation(&handle);
        let mut failures = FailureAccumulator::default();

        let creation_time = match first_creation {
            Ok(0) => {
                failures.record(OmissionKind::IdentityUnavailable);
                None
            }
            Ok(value) => Some(value),
            Err(failure) => {
                failures.record(failure.into());
                None
            }
        };
        failures.record_liveness(first_liveness);

        let (name, executable_path) = match observation {
            Ok(observation) => {
                failures.validate_path_observation(&self.adapter, &candidate, &observation);
                (Some(observation.name), Some(observation.executable_path))
            }
            Err(ObservationFailure::IdentityUnavailable) => {
                self.fallback_metadata(&candidate, &handle, creation_time, &mut failures)
            }
            Err(failure) => {
                failures.record(failure.into());
                (None, None)
            }
        };

        if let Some(kind) = failures.result() {
            return Err(kind);
        }
        let creation_time = creation_time.ok_or(OmissionKind::IdentityUnavailable)?;
        let name = name.ok_or(OmissionKind::IdentityUnavailable)?;
        let target = ProcessTarget::new(self.host_instance_id, candidate.pid, creation_time)
            .map_err(|_| OmissionKind::IdentityUnavailable)?;
        ProcessRecord::new(target, name, executable_path, candidate.observed_parent_pid)
            .map_err(|_| OmissionKind::IdentityUnavailable)
    }

    fn fallback_metadata(
        &self,
        candidate: &ProcessCandidate,
        handle: &A::Handle,
        first_creation: Option<u64>,
        failures: &mut FailureAccumulator,
    ) -> (Option<String>, Option<PathBuf>) {
        let name = match self.adapter.reenumerate_name(candidate.pid) {
            Ok(Some(name)) if valid_name(&name) => {
                failures.record_name_equivalence(
                    self.adapter
                        .process_names_equivalent(OsStr::new(&candidate.name), OsStr::new(&name)),
                );
                Some(name)
            }
            Ok(Some(_)) | Ok(None) => {
                failures.record(OmissionKind::IdentityUnavailable);
                None
            }
            Err(failure) => {
                failures.record(failure.into());
                None
            }
        };

        failures.record_liveness(self.adapter.liveness(handle));
        match self.adapter.creation_time(handle) {
            Ok(0) => failures.record(OmissionKind::IdentityUnavailable),
            Ok(second_creation) => match first_creation {
                Some(first_creation) if first_creation == second_creation => {}
                Some(_) => failures.record(OmissionKind::ExitedOrChanged),
                None => {}
            },
            Err(failure) => failures.record(failure.into()),
        }
        (name, None)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum OmissionKind {
    IdentityUnavailable,
    InspectionDenied,
    ExitedOrChanged,
}

impl From<ObservationFailure> for OmissionKind {
    fn from(failure: ObservationFailure) -> Self {
        match failure {
            ObservationFailure::ExitedOrChanged => Self::ExitedOrChanged,
            ObservationFailure::AccessDenied => Self::InspectionDenied,
            ObservationFailure::IdentityUnavailable => Self::IdentityUnavailable,
        }
    }
}

#[derive(Default)]
struct FailureAccumulator {
    highest: Option<OmissionKind>,
}

impl FailureAccumulator {
    fn record(&mut self, kind: OmissionKind) {
        self.highest = Some(self.highest.map_or(kind, |current| current.max(kind)));
    }

    fn record_liveness(&mut self, liveness: Result<Liveness, ObservationFailure>) {
        match liveness {
            Ok(Liveness::Alive) => {}
            Ok(Liveness::Exited) => self.record(OmissionKind::ExitedOrChanged),
            Err(failure) => self.record(failure.into()),
        }
    }

    fn validate_path_observation<A: ProcessPlatformAdapter>(
        &mut self,
        adapter: &A,
        candidate: &ProcessCandidate,
        observation: &ProcessObservation,
    ) {
        if !valid_name(&observation.name) || !valid_path(&observation.executable_path) {
            self.record(OmissionKind::IdentityUnavailable);
            return;
        }
        let Some(path_name) = observation.executable_path.file_name() else {
            self.record(OmissionKind::IdentityUnavailable);
            return;
        };
        let observation_name = OsStr::new(&observation.name);
        self.record_name_equivalence(
            adapter.process_names_equivalent(OsStr::new(&candidate.name), observation_name),
        );
        self.record_name_equivalence(adapter.process_names_equivalent(path_name, observation_name));
    }

    fn record_name_equivalence(&mut self, equivalence: Result<bool, ObservationFailure>) {
        match equivalence {
            Ok(true) => {}
            Ok(false) => self.record(OmissionKind::ExitedOrChanged),
            Err(failure) => self.record(failure.into()),
        }
    }

    fn result(self) -> Option<OmissionKind> {
        self.highest
    }
}

pub(super) struct CoreAdmission<H> {
    target: ProcessTarget,
    intent: ProcessIntent,
    _handle: H,
}

impl<H> CoreAdmission<H> {
    pub(super) fn target(&self) -> &ProcessTarget {
        &self.target
    }

    pub(super) fn intent(&self) -> ProcessIntent {
        self.intent
    }
}

fn admission_identity_failure(failure: ObservationFailure) -> ProcessError {
    match failure {
        ObservationFailure::ExitedOrChanged => ProcessError::TargetExited,
        ObservationFailure::AccessDenied | ObservationFailure::IdentityUnavailable => {
            ProcessError::InspectionDenied
        }
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name == name.trim() && !name.contains(['\0', '\\', '/'])
}

fn valid_path(path: &Path) -> bool {
    let value = path.as_os_str().to_string_lossy();
    !value.is_empty() && !value.contains('\0')
}
