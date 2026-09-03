use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    ffi::OsStr,
    path::PathBuf,
    rc::Rc,
};

use super::core::{
    CoreAuthority, EnumerationFailure, HostInstanceId, Liveness, ObservationAccess,
    ObservationFailure, ProcessCandidate, ProcessError, ProcessIntent, ProcessObservation,
    ProcessPlatformAdapter, ProcessProtection, ProcessTarget,
};

const HOST: [u8; 16] = [0x41; 16];
const SELF_PID: u32 = 9_999;
const SELF_CREATION_TIME: u64 = 99_990;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FakeCall {
    Enumerate,
    Open(u32),
    CreationTime(u32),
    Liveness(u32),
    Observation(u32),
    ReenumerateName(u32),
    ProcessNamesEquivalent,
    Protection(u32),
}

#[derive(Clone)]
enum FakeMutation {
    CreationTimes {
        pid: u32,
        values: Vec<Result<u64, ObservationFailure>>,
    },
    Liveness {
        pid: u32,
        values: Vec<Result<Liveness, ObservationFailure>>,
    },
    Names {
        pid: u32,
        values: Vec<Result<Option<String>, ObservationFailure>>,
    },
}

struct MutationHook {
    after: FakeCall,
    occurrence: usize,
    mutation: FakeMutation,
}

#[derive(Default)]
struct FakeProcessScript {
    open: VecDeque<Result<(), ObservationFailure>>,
    creation_times: VecDeque<Result<u64, ObservationFailure>>,
    liveness: VecDeque<Result<Liveness, ObservationFailure>>,
    observations: VecDeque<Result<ProcessObservation, ObservationFailure>>,
    reenumerated_names: VecDeque<Result<Option<String>, ObservationFailure>>,
    protections: VecDeque<Result<ProcessProtection, ObservationFailure>>,
}

struct FakeProcessObject {
    id: u64,
    script: FakeProcessScript,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FakeHandleEvent {
    Opened {
        handle_id: u64,
        object_id: u64,
        pid: u32,
    },
    Dropped {
        handle_id: u64,
        object_id: u64,
        pid: u32,
    },
}

struct FakeState {
    enumerations:
        VecDeque<Result<Vec<Result<ProcessCandidate, EnumerationFailure>>, EnumerationFailure>>,
    processes: BTreeMap<u32, Rc<RefCell<FakeProcessObject>>>,
    calls: Vec<FakeCall>,
    hooks: Vec<MutationHook>,
    requested_access: Vec<ObservationAccess>,
    process_name_equivalences: VecDeque<Result<bool, ObservationFailure>>,
    opened: usize,
    next_handle_id: u64,
    next_object_id: u64,
    dropped: BTreeSet<u64>,
    handle_events: Vec<FakeHandleEvent>,
}

impl Default for FakeState {
    fn default() -> Self {
        Self {
            enumerations: VecDeque::new(),
            processes: BTreeMap::new(),
            calls: Vec::new(),
            hooks: Vec::new(),
            requested_access: Vec::new(),
            process_name_equivalences: VecDeque::new(),
            opened: 0,
            next_handle_id: 1,
            next_object_id: 1,
            dropped: BTreeSet::new(),
            handle_events: Vec::new(),
        }
    }
}

#[derive(Clone, Default)]
struct FakeProcessAdapter {
    state: Rc<RefCell<FakeState>>,
}

struct FakeHandle {
    id: u64,
    pid: u32,
    object: Rc<RefCell<FakeProcessObject>>,
    state: Rc<RefCell<FakeState>>,
}

impl Drop for FakeHandle {
    fn drop(&mut self) {
        let object_id = self.object.borrow().id;
        let mut state = self.state.borrow_mut();
        let inserted = state.dropped.insert(self.id);
        assert!(
            inserted,
            "fake handle {} was dropped more than once",
            self.id
        );
        state.handle_events.push(FakeHandleEvent::Dropped {
            handle_id: self.id,
            object_id,
            pid: self.pid,
        });
    }
}

impl FakeProcessAdapter {
    fn with_enumeration(
        self,
        candidates: Vec<Result<ProcessCandidate, EnumerationFailure>>,
    ) -> Self {
        self.state
            .borrow_mut()
            .enumerations
            .push_back(Ok(candidates));
        self
    }

    fn with_enumeration_failure(self) -> Self {
        self.state
            .borrow_mut()
            .enumerations
            .push_back(Err(EnumerationFailure::Unavailable));
        self
    }

    fn with_process(self, pid: u32, script: FakeProcessScript) -> Self {
        self.replace_process(pid, script);
        self
    }

    fn replace_process(&self, pid: u32, script: FakeProcessScript) -> u64 {
        let mut state = self.state.borrow_mut();
        let object_id = state.next_object_id;
        state.next_object_id += 1;
        state.processes.insert(
            pid,
            Rc::new(RefCell::new(FakeProcessObject {
                id: object_id,
                script,
            })),
        );
        object_id
    }

    fn with_process_name_equivalences(self, values: Vec<Result<bool, ObservationFailure>>) -> Self {
        self.state.borrow_mut().process_name_equivalences = values.into();
        self
    }

    fn mutate_after(self, after: FakeCall, occurrence: usize, mutation: FakeMutation) -> Self {
        self.state.borrow_mut().hooks.push(MutationHook {
            after,
            occurrence,
            mutation,
        });
        self
    }

    fn calls(&self) -> Vec<FakeCall> {
        self.state.borrow().calls.clone()
    }

    fn opened(&self) -> usize {
        self.state.borrow().opened
    }

    fn dropped(&self) -> usize {
        self.state.borrow().dropped.len()
    }

    fn handle_events(&self) -> Vec<FakeHandleEvent> {
        self.state.borrow().handle_events.clone()
    }

    fn requested_access(&self) -> Vec<ObservationAccess> {
        self.state.borrow().requested_access.clone()
    }

    fn finish_call(&self, call: FakeCall) {
        let mutations = {
            let mut state = self.state.borrow_mut();
            state.calls.push(call);
            let occurrence = state.calls.iter().filter(|entry| **entry == call).count();
            let mut mutations = Vec::new();
            let mut index = 0;
            while index < state.hooks.len() {
                if state.hooks[index].after == call && state.hooks[index].occurrence == occurrence {
                    mutations.push(state.hooks.remove(index).mutation);
                } else {
                    index += 1;
                }
            }
            mutations
        };
        for mutation in mutations {
            self.apply_mutation(mutation);
        }
    }

    fn apply_mutation(&self, mutation: FakeMutation) {
        match mutation {
            FakeMutation::CreationTimes { pid, values } => {
                self.current_process(pid).borrow_mut().script.creation_times = values.into();
            }
            FakeMutation::Liveness { pid, values } => {
                self.current_process(pid).borrow_mut().script.liveness = values.into();
            }
            FakeMutation::Names { pid, values } => {
                self.current_process(pid)
                    .borrow_mut()
                    .script
                    .reenumerated_names = values.into();
            }
        }
    }

    fn current_process(&self, pid: u32) -> Rc<RefCell<FakeProcessObject>> {
        Rc::clone(
            self.state
                .borrow()
                .processes
                .get(&pid)
                .expect("fake process must exist"),
        )
    }

    fn pop_process_value<T>(
        object: &Rc<RefCell<FakeProcessObject>>,
        select: impl FnOnce(&mut FakeProcessScript) -> &mut VecDeque<Result<T, ObservationFailure>>,
    ) -> Result<T, ObservationFailure> {
        select(&mut object.borrow_mut().script)
            .pop_front()
            .unwrap_or(Err(ObservationFailure::IdentityUnavailable))
    }

    fn pop_current_process_value<T>(
        &self,
        pid: u32,
        select: impl FnOnce(&mut FakeProcessScript) -> &mut VecDeque<Result<T, ObservationFailure>>,
    ) -> Result<T, ObservationFailure> {
        Self::pop_process_value(&self.current_process(pid), select)
    }
}

impl ProcessPlatformAdapter for FakeProcessAdapter {
    type Handle = FakeHandle;

    fn enumerate_candidates(
        &self,
    ) -> Result<Vec<Result<ProcessCandidate, EnumerationFailure>>, EnumerationFailure> {
        let result = self
            .state
            .borrow_mut()
            .enumerations
            .pop_front()
            .unwrap_or(Err(EnumerationFailure::Unavailable));
        self.finish_call(FakeCall::Enumerate);
        result
    }

    fn open_observation(
        &self,
        pid: u32,
        access: ObservationAccess,
    ) -> Result<Self::Handle, ObservationFailure> {
        self.state.borrow_mut().requested_access.push(access);
        let object = self.current_process(pid);
        let outcome = Self::pop_process_value(&object, |script| &mut script.open);
        let result = outcome.map(|()| {
            let object_id = object.borrow().id;
            let mut state = self.state.borrow_mut();
            let id = state.next_handle_id;
            state.next_handle_id += 1;
            state.opened += 1;
            state.handle_events.push(FakeHandleEvent::Opened {
                handle_id: id,
                object_id,
                pid,
            });
            FakeHandle {
                id,
                pid,
                object,
                state: Rc::clone(&self.state),
            }
        });
        self.finish_call(FakeCall::Open(pid));
        result
    }

    fn creation_time(&self, handle: &Self::Handle) -> Result<u64, ObservationFailure> {
        let result = Self::pop_process_value(&handle.object, |script| &mut script.creation_times);
        self.finish_call(FakeCall::CreationTime(handle.pid));
        result
    }

    fn liveness(&self, handle: &Self::Handle) -> Result<Liveness, ObservationFailure> {
        let result = Self::pop_process_value(&handle.object, |script| &mut script.liveness);
        self.finish_call(FakeCall::Liveness(handle.pid));
        result
    }

    fn observation(&self, handle: &Self::Handle) -> Result<ProcessObservation, ObservationFailure> {
        let result = Self::pop_process_value(&handle.object, |script| &mut script.observations);
        self.finish_call(FakeCall::Observation(handle.pid));
        result
    }

    fn reenumerate_name(&self, pid: u32) -> Result<Option<String>, ObservationFailure> {
        let result = self.pop_current_process_value(pid, |script| &mut script.reenumerated_names);
        self.finish_call(FakeCall::ReenumerateName(pid));
        result
    }

    fn process_names_equivalent(
        &self,
        left: &OsStr,
        right: &OsStr,
    ) -> Result<bool, ObservationFailure> {
        let result = self
            .state
            .borrow_mut()
            .process_name_equivalences
            .pop_front()
            .unwrap_or_else(|| Ok(left == right));
        self.finish_call(FakeCall::ProcessNamesEquivalent);
        result
    }

    fn protection(&self, handle: &Self::Handle) -> Result<ProcessProtection, ObservationFailure> {
        let result = Self::pop_process_value(&handle.object, |script| &mut script.protections);
        self.finish_call(FakeCall::Protection(handle.pid));
        result
    }
}

fn candidate(pid: u32, name: &str, parent_pid: Option<u32>) -> ProcessCandidate {
    ProcessCandidate::new(pid, name.into(), parent_pid)
}

fn path_observation(name: &str, path: &str) -> ProcessObservation {
    ProcessObservation::new(name.into(), PathBuf::from(path))
}

fn successful_script(creation_time: u64, name: &str, path: &str) -> FakeProcessScript {
    FakeProcessScript {
        open: vec![Ok(())].into(),
        creation_times: vec![Ok(creation_time)].into(),
        liveness: vec![Ok(Liveness::Alive)].into(),
        observations: vec![Ok(path_observation(name, path))].into(),
        ..FakeProcessScript::default()
    }
}

fn authority(adapter: FakeProcessAdapter) -> CoreAuthority<FakeProcessAdapter> {
    authority_with_self(adapter, SELF_PID, SELF_CREATION_TIME)
}

fn authority_with_self(
    adapter: FakeProcessAdapter,
    self_pid: u32,
    self_creation_time: u64,
) -> CoreAuthority<FakeProcessAdapter> {
    let host = HostInstanceId::new(HOST).unwrap();
    let self_target = ProcessTarget::new(host, self_pid, self_creation_time).unwrap();
    CoreAuthority::new(host, self_target, adapter)
}

fn target(pid: u32, creation_time: u64) -> ProcessTarget {
    ProcessTarget::new(HostInstanceId::new(HOST).unwrap(), pid, creation_time).unwrap()
}

fn admission_script(
    creation_time: u64,
    liveness: Result<Liveness, ObservationFailure>,
    protection: Result<ProcessProtection, ObservationFailure>,
) -> FakeProcessScript {
    FakeProcessScript {
        open: vec![Ok(())].into(),
        creation_times: vec![Ok(creation_time)].into(),
        liveness: vec![liveness].into(),
        protections: vec![protection].into(),
        ..FakeProcessScript::default()
    }
}

#[test]
fn snapshot_orders_reliable_records_by_pid_and_requests_only_observation_access() {
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![
            Ok(candidate(42, "zeta.exe", Some(7))),
            Ok(candidate(7, "alpha.exe", None)),
        ])
        .with_process(
            42,
            successful_script(4200, "zeta.exe", r"C:\Tools\zeta.exe"),
        )
        .with_process(7, successful_script(700, "alpha.exe", r"C:\Apps\alpha.exe"));
    let authority = authority(adapter.clone());

    let snapshot = authority.snapshot().unwrap();

    assert_eq!(
        snapshot
            .processes()
            .iter()
            .map(|record| record.pid())
            .collect::<Vec<_>>(),
        vec![7, 42]
    );
    assert_eq!(snapshot.processes()[0].name(), "alpha.exe");
    assert_eq!(
        snapshot.processes()[1].executable_path(),
        Some(PathBuf::from(r"C:\Tools\zeta.exe").as_path())
    );
    assert_eq!(snapshot.processes()[1].observed_parent_pid(), Some(7));
    assert_eq!(snapshot.omissions().total(), 0);
    assert_eq!(
        adapter.requested_access(),
        vec![ObservationAccess::SNAPSHOT, ObservationAccess::SNAPSHOT]
    );
    assert_eq!(adapter.opened(), 2);
    assert_eq!(adapter.dropped(), 2);
}

#[test]
fn snapshot_accepts_platform_equivalent_non_ascii_candidate_and_path_names() {
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![Ok(candidate(6, "ÖRBIT.EXE", None))])
        .with_process(6, successful_script(600, "örbit.exe", r"C:\Apps\Örbit.Exe"))
        .with_process_name_equivalences(vec![Ok(true), Ok(true)]);

    let snapshot = authority(adapter.clone()).snapshot().unwrap();

    assert_eq!(snapshot.processes().len(), 1);
    assert_eq!(snapshot.processes()[0].name(), "örbit.exe");
    assert_eq!(snapshot.omissions().total(), 0);
    assert_eq!(
        adapter
            .calls()
            .iter()
            .filter(|call| **call == FakeCall::ProcessNamesEquivalent)
            .count(),
        2
    );
    assert_eq!(adapter.opened(), adapter.dropped());
}

#[test]
fn snapshot_duplicate_candidate_is_one_changed_omission_without_a_second_open() {
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![
            Ok(candidate(8, "same.exe", None)),
            Ok(candidate(8, "changed.exe", None)),
        ])
        .with_process(8, successful_script(800, "same.exe", r"C:\Apps\same.exe"));

    let snapshot = authority(adapter.clone()).snapshot().unwrap();

    assert_eq!(snapshot.processes().len(), 1);
    assert_eq!(snapshot.omissions().exited_or_changed(), 1);
    assert_eq!(snapshot.omissions().total(), 1);
    assert_eq!(adapter.opened(), 1);
    assert_eq!(adapter.dropped(), 1);
}

#[test]
fn snapshot_duplicate_candidate_valid_then_invalid_prioritizes_changed() {
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![
            Ok(candidate(9, "same.exe", None)),
            Ok(candidate(9, "", None)),
        ])
        .with_process(9, successful_script(900, "same.exe", r"C:\Apps\same.exe"));

    let snapshot = authority(adapter.clone()).snapshot().unwrap();

    assert_eq!(snapshot.processes().len(), 1);
    assert_eq!(snapshot.omissions().exited_or_changed(), 1);
    assert_eq!(snapshot.omissions().identity_unavailable(), 0);
    assert_eq!(
        snapshot.processes().len() as u64 + snapshot.omissions().total(),
        2
    );
    assert_eq!(adapter.opened(), 1);
    assert_eq!(adapter.dropped(), 1);
}

#[test]
fn snapshot_duplicate_candidate_invalid_then_valid_records_both_as_omissions() {
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![
            Ok(candidate(10, "", None)),
            Ok(candidate(10, "same.exe", None)),
        ])
        .with_process(10, successful_script(1000, "same.exe", r"C:\Apps\same.exe"));

    let snapshot = authority(adapter.clone()).snapshot().unwrap();

    assert!(snapshot.processes().is_empty());
    assert_eq!(snapshot.omissions().exited_or_changed(), 1);
    assert_eq!(snapshot.omissions().identity_unavailable(), 1);
    assert_eq!(snapshot.omissions().total(), 2);
    assert_eq!(adapter.opened(), 0);
    assert_eq!(adapter.dropped(), 0);
}

#[test]
fn snapshot_zero_pid_candidates_remain_identity_unavailable_not_duplicates() {
    let adapter = FakeProcessAdapter::default().with_enumeration(vec![
        Ok(candidate(0, "", None)),
        Ok(candidate(0, "special.exe", None)),
    ]);

    let snapshot = authority(adapter.clone()).snapshot().unwrap();

    assert!(snapshot.processes().is_empty());
    assert_eq!(snapshot.omissions().exited_or_changed(), 0);
    assert_eq!(snapshot.omissions().identity_unavailable(), 2);
    assert_eq!(snapshot.omissions().total(), 2);
    assert_eq!(adapter.opened(), 0);
    assert_eq!(adapter.dropped(), 0);
}

#[test]
fn snapshot_path_unavailable_reenumerates_name_then_revalidates_the_open_handle() {
    let script = FakeProcessScript {
        open: vec![Ok(())].into(),
        creation_times: vec![Ok(1100), Ok(1100)].into(),
        liveness: vec![Ok(Liveness::Alive), Ok(Liveness::Alive)].into(),
        observations: vec![Err(ObservationFailure::IdentityUnavailable)].into(),
        reenumerated_names: vec![Ok(Some("fallback.exe".into()))].into(),
        ..FakeProcessScript::default()
    };
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![Ok(candidate(11, "fallback.exe", Some(1)))])
        .with_process(11, script);

    let snapshot = authority(adapter.clone()).snapshot().unwrap();

    assert_eq!(snapshot.processes().len(), 1);
    assert_eq!(snapshot.processes()[0].name(), "fallback.exe");
    assert_eq!(snapshot.processes()[0].executable_path(), None);
    assert_eq!(
        adapter.calls(),
        vec![
            FakeCall::Enumerate,
            FakeCall::Open(11),
            FakeCall::CreationTime(11),
            FakeCall::Liveness(11),
            FakeCall::Observation(11),
            FakeCall::ReenumerateName(11),
            FakeCall::ProcessNamesEquivalent,
            FakeCall::Liveness(11),
            FakeCall::CreationTime(11),
        ]
    );
    assert_eq!(adapter.opened(), adapter.dropped());
}

#[test]
fn snapshot_path_fallback_omits_a_handle_that_changes_after_name_reenumeration() {
    let script = FakeProcessScript {
        open: vec![Ok(())].into(),
        creation_times: vec![Ok(1200)].into(),
        liveness: vec![Ok(Liveness::Alive), Ok(Liveness::Alive)].into(),
        observations: vec![Err(ObservationFailure::IdentityUnavailable)].into(),
        reenumerated_names: vec![Ok(Some("fallback.exe".into()))].into(),
        ..FakeProcessScript::default()
    };
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![Ok(candidate(12, "fallback.exe", None))])
        .with_process(12, script)
        .mutate_after(
            FakeCall::ReenumerateName(12),
            1,
            FakeMutation::CreationTimes {
                pid: 12,
                values: vec![Ok(1201)],
            },
        );

    let snapshot = authority(adapter.clone()).snapshot().unwrap();

    assert!(snapshot.processes().is_empty());
    assert_eq!(snapshot.omissions().exited_or_changed(), 1);
    assert_eq!(snapshot.omissions().inspection_denied(), 0);
    assert_eq!(snapshot.omissions().identity_unavailable(), 0);
    assert_eq!(adapter.opened(), adapter.dropped());
}

#[test]
fn snapshot_omission_priority_is_changed_then_denied_then_identity_unavailable() {
    let changed = FakeProcessScript {
        open: vec![Ok(())].into(),
        creation_times: vec![Err(ObservationFailure::AccessDenied)].into(),
        liveness: vec![Ok(Liveness::Exited)].into(),
        observations: vec![Err(ObservationFailure::IdentityUnavailable)].into(),
        reenumerated_names: vec![Err(ObservationFailure::IdentityUnavailable)].into(),
        ..FakeProcessScript::default()
    };
    let denied = FakeProcessScript {
        open: vec![Ok(())].into(),
        creation_times: vec![Err(ObservationFailure::AccessDenied)].into(),
        liveness: vec![Ok(Liveness::Alive)].into(),
        observations: vec![Err(ObservationFailure::IdentityUnavailable)].into(),
        reenumerated_names: vec![Err(ObservationFailure::IdentityUnavailable)].into(),
        ..FakeProcessScript::default()
    };
    let unavailable = FakeProcessScript {
        open: vec![Ok(())].into(),
        creation_times: vec![Err(ObservationFailure::IdentityUnavailable)].into(),
        liveness: vec![Ok(Liveness::Alive)].into(),
        observations: vec![Err(ObservationFailure::IdentityUnavailable)].into(),
        reenumerated_names: vec![Err(ObservationFailure::IdentityUnavailable)].into(),
        ..FakeProcessScript::default()
    };
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![
            Ok(candidate(20, "changed.exe", None)),
            Ok(candidate(21, "denied.exe", None)),
            Ok(candidate(22, "unknown.exe", None)),
        ])
        .with_process(20, changed)
        .with_process(21, denied)
        .with_process(22, unavailable);

    let snapshot = authority(adapter.clone()).snapshot().unwrap();

    assert!(snapshot.processes().is_empty());
    assert_eq!(snapshot.omissions().exited_or_changed(), 1);
    assert_eq!(snapshot.omissions().inspection_denied(), 1);
    assert_eq!(snapshot.omissions().identity_unavailable(), 1);
    assert_eq!(snapshot.omissions().total(), 3);
    assert_eq!(adapter.opened(), 3);
    assert_eq!(adapter.dropped(), 3);
}

#[test]
fn snapshot_entry_failure_preserves_other_reliable_records() {
    let denied = FakeProcessScript {
        open: vec![Err(ObservationFailure::AccessDenied)].into(),
        ..FakeProcessScript::default()
    };
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![
            Ok(candidate(30, "denied.exe", None)),
            Ok(candidate(31, "kept.exe", None)),
        ])
        .with_process(30, denied)
        .with_process(31, successful_script(3100, "kept.exe", r"C:\Apps\kept.exe"));

    let snapshot = authority(adapter.clone()).snapshot().unwrap();

    assert_eq!(snapshot.processes().len(), 1);
    assert_eq!(snapshot.processes()[0].pid(), 31);
    assert_eq!(snapshot.omissions().inspection_denied(), 1);
    assert_eq!(adapter.opened(), 1);
    assert_eq!(adapter.dropped(), 1);
}

#[test]
fn snapshot_creation_failure_returns_catalog_unavailable_without_opening_candidates() {
    let adapter = FakeProcessAdapter::default().with_enumeration_failure();

    assert_eq!(
        authority(adapter.clone()).snapshot(),
        Err(ProcessError::CatalogUnavailable)
    );
    assert_eq!(adapter.opened(), 0);
    assert_eq!(adapter.dropped(), 0);
}

#[test]
fn snapshot_mid_enumeration_failure_returns_catalog_unavailable_without_partial_work() {
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![
            Ok(candidate(40, "would-have-opened.exe", None)),
            Err(EnumerationFailure::Unavailable),
        ])
        .with_process(
            40,
            successful_script(
                4000,
                "would-have-opened.exe",
                r"C:\Apps\would-have-opened.exe",
            ),
        );

    assert_eq!(
        authority(adapter.clone()).snapshot(),
        Err(ProcessError::CatalogUnavailable)
    );
    assert_eq!(adapter.opened(), 0);
    assert_eq!(adapter.dropped(), 0);
}

#[test]
fn snapshot_refresh_preserves_the_same_target_for_an_unchanged_process() {
    let mut script = successful_script(5000, "steady.exe", r"C:\Apps\steady.exe");
    script.open.push_back(Ok(()));
    script.creation_times.push_back(Ok(5000));
    script.liveness.push_back(Ok(Liveness::Alive));
    script
        .observations
        .push_back(Ok(path_observation("steady.exe", r"D:\Moved\steady.exe")));
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![Ok(candidate(50, "steady.exe", Some(3)))])
        .with_enumeration(vec![Ok(candidate(50, "steady.exe", Some(9)))])
        .with_process(50, script);
    let authority = authority(adapter.clone());

    let first = authority.snapshot().unwrap();
    let second = authority.snapshot().unwrap();

    assert_eq!(
        first.processes()[0].target(),
        second.processes()[0].target()
    );
    assert_ne!(
        first.processes()[0].executable_path(),
        second.processes()[0].executable_path()
    );
    assert_ne!(
        first.processes()[0].observed_parent_pid(),
        second.processes()[0].observed_parent_pid()
    );
    assert_eq!(adapter.opened(), 2);
    assert_eq!(adapter.dropped(), 2);
}

#[test]
fn snapshot_target_equality_never_includes_display_metadata() {
    let host = HostInstanceId::new(HOST).unwrap();
    let first = ProcessTarget::new(host, 60, 6000).unwrap();
    let second = ProcessTarget::new(host, 60, 6000).unwrap();

    assert_eq!(first, second);
    assert_eq!(first.host_instance_id(), host);
    assert_eq!(first.pid(), 60);
    assert_eq!(first.creation_time(), 6000);
}

#[test]
fn snapshot_every_temporary_handle_closes_once_across_success_and_failures() {
    let unavailable_path_then_exit = FakeProcessScript {
        open: vec![Ok(())].into(),
        creation_times: vec![Ok(7100), Ok(7100)].into(),
        liveness: vec![Ok(Liveness::Alive), Ok(Liveness::Exited)].into(),
        observations: vec![Err(ObservationFailure::IdentityUnavailable)].into(),
        reenumerated_names: vec![Ok(Some("gone.exe".into()))].into(),
        ..FakeProcessScript::default()
    };
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![
            Ok(candidate(70, "kept.exe", None)),
            Ok(candidate(71, "gone.exe", None)),
            Ok(candidate(72, "denied.exe", None)),
        ])
        .with_process(70, successful_script(7000, "kept.exe", r"C:\Apps\kept.exe"))
        .with_process(71, unavailable_path_then_exit)
        .with_process(
            72,
            FakeProcessScript {
                open: vec![Err(ObservationFailure::AccessDenied)].into(),
                ..FakeProcessScript::default()
            },
        );

    let snapshot = authority(adapter.clone()).snapshot().unwrap();

    assert_eq!(snapshot.processes().len(), 1);
    assert_eq!(snapshot.omissions().exited_or_changed(), 1);
    assert_eq!(snapshot.omissions().inspection_denied(), 1);
    assert_eq!(adapter.opened(), 2);
    assert_eq!(adapter.dropped(), 2);
}

#[test]
fn snapshot_value_constructors_and_stable_error_strings_are_strict() {
    assert!(HostInstanceId::new([0; 16]).is_err());
    let host = HostInstanceId::new(HOST).unwrap();
    assert_eq!(
        ProcessTarget::new(host, 0, 1),
        Err(ProcessError::InvalidTarget)
    );
    assert_eq!(
        ProcessTarget::new(host, 1, 0),
        Err(ProcessError::InvalidTarget)
    );
    assert_eq!(
        [
            ProcessError::CatalogUnavailable,
            ProcessError::InvalidTarget,
            ProcessError::ExpiredTarget,
            ProcessError::TargetExited,
            ProcessError::TargetChanged,
            ProcessError::InspectionDenied,
            ProcessError::ProtectedTarget,
            ProcessError::PlatformUnsupported,
        ]
        .map(|error| error.to_string()),
        [
            "CatalogUnavailable",
            "InvalidTarget",
            "ExpiredTarget",
            "TargetExited",
            "TargetChanged",
            "InspectionDenied",
            "ProtectedTarget",
            "PlatformUnsupported",
        ]
    );
}

#[test]
fn snapshot_stage_one_source_has_no_tauri_or_frontend_wiring() {
    let lib = include_str!("../lib.rs").replace("\r\n", "\n");
    let production = lib
        .split("#[cfg(test)]\nmod tests")
        .next()
        .expect("test module marker is missing");
    let process_module = include_str!("mod.rs");
    let process_core = include_str!("core.rs");

    assert_eq!(production.matches("process_control").count(), 1);
    assert_eq!(
        production
            .matches("reason = \"Stage 2 supplies the first production caller\"")
            .count(),
        1
    );
    for forbidden in [
        ".manage(process_control",
        "generate_handler![process_control",
        "process_control::command",
        "process_control::build_capability",
        "process_control::register_frontend",
    ] {
        assert!(!production.contains(forbidden));
    }
    for forbidden in [
        "#[tauri::command]",
        "tauri::Builder",
        ".manage(",
        "invoke_handler",
        "generate_handler!",
        "build_capability",
        "register_frontend",
    ] {
        assert!(!process_module.contains(forbidden));
        assert!(!process_core.contains(forbidden));
    }
}

#[test]
fn snapshot_fake_mutation_hooks_can_change_each_scripted_identity_signal() {
    let script = FakeProcessScript {
        open: vec![Ok(())].into(),
        creation_times: vec![Ok(8000), Ok(8000)].into(),
        liveness: vec![Ok(Liveness::Alive), Ok(Liveness::Alive)].into(),
        observations: vec![Err(ObservationFailure::IdentityUnavailable)].into(),
        reenumerated_names: vec![Ok(Some("before.exe".into()))].into(),
        ..FakeProcessScript::default()
    };
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![Ok(candidate(80, "before.exe", None))])
        .with_process(80, script)
        .mutate_after(
            FakeCall::Observation(80),
            1,
            FakeMutation::Names {
                pid: 80,
                values: vec![Ok(Some("after.exe".into()))],
            },
        )
        .mutate_after(
            FakeCall::ReenumerateName(80),
            1,
            FakeMutation::Liveness {
                pid: 80,
                values: vec![Ok(Liveness::Exited)],
            },
        );

    let snapshot = authority(adapter.clone()).snapshot().unwrap();

    assert!(snapshot.processes().is_empty());
    assert_eq!(snapshot.omissions().exited_or_changed(), 1);
    assert_eq!(adapter.opened(), adapter.dropped());
}

#[test]
fn admission_rejects_invalid_target_structure_before_any_adapter_call() {
    for invalid in [
        ProcessTarget::from_parts_for_test([0; 16], 91, 9_100),
        ProcessTarget::from_parts_for_test(HOST, 0, 9_100),
        ProcessTarget::from_parts_for_test(HOST, 91, 0),
    ] {
        let adapter = FakeProcessAdapter::default();

        assert_eq!(
            authority(adapter.clone())
                .admit(&invalid, ProcessIntent::Terminate)
                .err(),
            Some(ProcessError::InvalidTarget)
        );
        assert!(adapter.calls().is_empty());
        assert_eq!(adapter.opened(), 0);
        assert_eq!(adapter.dropped(), 0);
    }
}

#[test]
fn admission_rejects_an_expired_host_without_calling_the_adapter() {
    let adapter = FakeProcessAdapter::default();
    let stale = ProcessTarget::new(HostInstanceId::new([0x42; 16]).unwrap(), 92, 9_200).unwrap();

    assert_eq!(
        authority(adapter.clone())
            .admit(&stale, ProcessIntent::Terminate)
            .err(),
        Some(ProcessError::ExpiredTarget)
    );
    assert!(adapter.calls().is_empty());
    assert!(adapter.requested_access().is_empty());
    assert_eq!(adapter.opened(), 0);
    assert_eq!(adapter.dropped(), 0);
}

#[test]
fn admission_rejects_pid_reuse_before_liveness_or_replacement_protection() {
    let pid = 93;
    let script = admission_script(
        9_301,
        Ok(Liveness::Exited),
        Ok(ProcessProtection::new(true, true)),
    );
    let adapter = FakeProcessAdapter::default().with_process(pid, script);

    assert_eq!(
        authority(adapter.clone())
            .admit(&target(pid, 9_300), ProcessIntent::ForceTerminate)
            .err(),
        Some(ProcessError::TargetChanged)
    );
    assert_eq!(
        adapter.calls(),
        vec![FakeCall::Open(pid), FakeCall::CreationTime(pid)]
    );
    assert_eq!(adapter.opened(), 1);
    assert_eq!(adapter.dropped(), 1);
}

#[test]
fn admission_maps_exit_before_open_to_target_exited_without_a_handle() {
    let pid = 94;
    let adapter = FakeProcessAdapter::default().with_process(
        pid,
        FakeProcessScript {
            open: vec![Err(ObservationFailure::ExitedOrChanged)].into(),
            ..FakeProcessScript::default()
        },
    );

    assert_eq!(
        authority(adapter.clone())
            .admit(&target(pid, 9_400), ProcessIntent::Restart)
            .err(),
        Some(ProcessError::TargetExited)
    );
    assert_eq!(adapter.calls(), vec![FakeCall::Open(pid)]);
    assert_eq!(adapter.opened(), 0);
    assert_eq!(adapter.dropped(), 0);
}

#[test]
fn admission_maps_observation_open_denials_without_opening_or_continuing() {
    for (offset, failure) in [
        ObservationFailure::AccessDenied,
        ObservationFailure::IdentityUnavailable,
    ]
    .into_iter()
    .enumerate()
    {
        let pid = 940 + offset as u32;
        let creation_time = 9_400 + offset as u64;
        let adapter = FakeProcessAdapter::default().with_process(
            pid,
            FakeProcessScript {
                open: vec![Err(failure)].into(),
                creation_times: vec![Ok(creation_time)].into(),
                liveness: vec![Ok(Liveness::Alive)].into(),
                protections: vec![Ok(ProcessProtection::new(false, false))].into(),
                ..FakeProcessScript::default()
            },
        );

        assert_eq!(
            authority(adapter.clone())
                .admit(&target(pid, creation_time), ProcessIntent::Terminate)
                .err(),
            Some(ProcessError::InspectionDenied)
        );
        assert_eq!(adapter.calls(), vec![FakeCall::Open(pid)]);
        assert_eq!(
            adapter.requested_access(),
            vec![ObservationAccess::SNAPSHOT]
        );
        assert_eq!(adapter.opened(), 0);
        assert_eq!(adapter.dropped(), 0);
    }
}

#[test]
fn admission_maps_creation_time_failures_and_closes_without_continuing() {
    for (offset, (creation_result, expected_error)) in [
        (
            Err(ObservationFailure::AccessDenied),
            ProcessError::InspectionDenied,
        ),
        (
            Err(ObservationFailure::IdentityUnavailable),
            ProcessError::InspectionDenied,
        ),
        (
            Err(ObservationFailure::ExitedOrChanged),
            ProcessError::TargetExited,
        ),
        (Ok(0), ProcessError::InspectionDenied),
    ]
    .into_iter()
    .enumerate()
    {
        let pid = 950 + offset as u32;
        let creation_time = 9_500 + offset as u64;
        let adapter = FakeProcessAdapter::default().with_process(
            pid,
            FakeProcessScript {
                open: vec![Ok(())].into(),
                creation_times: vec![creation_result].into(),
                liveness: vec![Ok(Liveness::Alive)].into(),
                protections: vec![Ok(ProcessProtection::new(false, false))].into(),
                ..FakeProcessScript::default()
            },
        );

        assert_eq!(
            authority(adapter.clone())
                .admit(&target(pid, creation_time), ProcessIntent::ForceTerminate)
                .err(),
            Some(expected_error)
        );
        assert_eq!(
            adapter.calls(),
            vec![FakeCall::Open(pid), FakeCall::CreationTime(pid)]
        );
        assert_eq!(
            adapter.requested_access(),
            vec![ObservationAccess::SNAPSHOT]
        );
        assert_eq!(adapter.opened(), 1);
        assert_eq!(adapter.dropped(), 1);
    }
}

#[test]
fn admission_maps_exit_after_open_to_target_exited_and_closes_the_handle() {
    let pid = 95;
    let adapter = FakeProcessAdapter::default().with_process(
        pid,
        admission_script(
            9_500,
            Ok(Liveness::Exited),
            Ok(ProcessProtection::new(false, false)),
        ),
    );

    assert_eq!(
        authority(adapter.clone())
            .admit(&target(pid, 9_500), ProcessIntent::Terminate)
            .err(),
        Some(ProcessError::TargetExited)
    );
    assert_eq!(
        adapter.calls(),
        vec![
            FakeCall::Open(pid),
            FakeCall::CreationTime(pid),
            FakeCall::Liveness(pid),
        ]
    );
    assert_eq!(adapter.opened(), 1);
    assert_eq!(adapter.dropped(), 1);
}

#[test]
fn admission_maps_liveness_inspection_failures_to_inspection_denied() {
    for (offset, failure) in [
        ObservationFailure::AccessDenied,
        ObservationFailure::IdentityUnavailable,
        ObservationFailure::ExitedOrChanged,
    ]
    .into_iter()
    .enumerate()
    {
        let pid = 960 + offset as u32;
        let creation_time = 9_600 + offset as u64;
        let adapter = FakeProcessAdapter::default().with_process(
            pid,
            admission_script(
                creation_time,
                Err(failure),
                Ok(ProcessProtection::new(false, false)),
            ),
        );

        assert_eq!(
            authority(adapter.clone())
                .admit(&target(pid, creation_time), ProcessIntent::ForceTerminate,)
                .err(),
            Some(ProcessError::InspectionDenied)
        );
        assert_eq!(
            adapter.calls(),
            vec![
                FakeCall::Open(pid),
                FakeCall::CreationTime(pid),
                FakeCall::Liveness(pid),
            ]
        );
        assert_eq!(adapter.opened(), 1);
        assert_eq!(adapter.dropped(), 1);
    }
}

#[test]
fn admission_fails_closed_when_protection_is_unreadable() {
    let pid = 97;
    let adapter = FakeProcessAdapter::default().with_process(
        pid,
        admission_script(
            9_700,
            Ok(Liveness::Alive),
            Err(ObservationFailure::AccessDenied),
        ),
    );

    assert_eq!(
        authority(adapter.clone())
            .admit(&target(pid, 9_700), ProcessIntent::Restart)
            .err(),
        Some(ProcessError::InspectionDenied)
    );
    assert_eq!(
        adapter.calls(),
        vec![
            FakeCall::Open(pid),
            FakeCall::CreationTime(pid),
            FakeCall::Liveness(pid),
            FakeCall::Protection(pid),
        ]
    );
    assert_eq!(adapter.opened(), 1);
    assert_eq!(adapter.dropped(), 1);
}

#[test]
fn admission_checks_protection_readability_before_rejecting_self() {
    let adapter = FakeProcessAdapter::default().with_process(
        SELF_PID,
        admission_script(
            SELF_CREATION_TIME,
            Ok(Liveness::Alive),
            Err(ObservationFailure::IdentityUnavailable),
        ),
    );

    assert_eq!(
        authority(adapter.clone())
            .admit(
                &target(SELF_PID, SELF_CREATION_TIME),
                ProcessIntent::Terminate,
            )
            .err(),
        Some(ProcessError::InspectionDenied)
    );
    assert_eq!(adapter.opened(), 1);
    assert_eq!(adapter.dropped(), 1);
}

#[test]
fn admission_rejects_the_exact_current_host_identity() {
    let adapter = FakeProcessAdapter::default().with_process(
        SELF_PID,
        admission_script(
            SELF_CREATION_TIME,
            Ok(Liveness::Alive),
            Ok(ProcessProtection::new(false, false)),
        ),
    );

    assert_eq!(
        authority(adapter.clone())
            .admit(
                &target(SELF_PID, SELF_CREATION_TIME),
                ProcessIntent::Terminate,
            )
            .err(),
        Some(ProcessError::ProtectedTarget)
    );
    assert_eq!(
        adapter.calls(),
        vec![
            FakeCall::Open(SELF_PID),
            FakeCall::CreationTime(SELF_PID),
            FakeCall::Liveness(SELF_PID),
            FakeCall::Protection(SELF_PID),
        ]
    );
    assert_eq!(adapter.opened(), 1);
    assert_eq!(adapter.dropped(), 1);
}

#[test]
fn admission_rejects_critical_and_protected_processes() {
    for (offset, protection) in [
        ProcessProtection::new(true, false),
        ProcessProtection::new(false, true),
        ProcessProtection::new(true, true),
    ]
    .into_iter()
    .enumerate()
    {
        let pid = 980 + offset as u32;
        let creation_time = 9_800 + offset as u64;
        let adapter = FakeProcessAdapter::default().with_process(
            pid,
            admission_script(creation_time, Ok(Liveness::Alive), Ok(protection)),
        );

        assert_eq!(
            authority(adapter.clone())
                .admit(&target(pid, creation_time), ProcessIntent::ForceTerminate,)
                .err(),
            Some(ProcessError::ProtectedTarget)
        );
        assert_eq!(adapter.opened(), 1);
        assert_eq!(adapter.dropped(), 1);
    }
}

#[test]
fn admission_binds_each_intent_and_uses_only_snapshot_observation_access() {
    let pid = 99;
    let creation_time = 9_900;
    let adapter = FakeProcessAdapter::default().with_process(
        pid,
        FakeProcessScript {
            open: vec![Ok(()), Ok(()), Ok(())].into(),
            creation_times: vec![Ok(creation_time), Ok(creation_time), Ok(creation_time)].into(),
            liveness: vec![
                Ok(Liveness::Alive),
                Ok(Liveness::Alive),
                Ok(Liveness::Alive),
            ]
            .into(),
            protections: vec![
                Ok(ProcessProtection::new(false, false)),
                Ok(ProcessProtection::new(false, false)),
                Ok(ProcessProtection::new(false, false)),
            ]
            .into(),
            ..FakeProcessScript::default()
        },
    );
    let authority = authority(adapter.clone());
    let target = target(pid, creation_time);

    for intent in [
        ProcessIntent::Terminate,
        ProcessIntent::ForceTerminate,
        ProcessIntent::Restart,
    ] {
        let admission = authority.admit(&target, intent).unwrap();
        assert_eq!(admission.target(), &target);
        assert_eq!(admission.intent(), intent);
        drop(admission);
    }

    assert_eq!(
        adapter.requested_access(),
        vec![
            ObservationAccess::SNAPSHOT,
            ObservationAccess::SNAPSHOT,
            ObservationAccess::SNAPSHOT,
        ]
    );
    assert!(adapter
        .requested_access()
        .into_iter()
        .all(|access| access.query_limited_information() && access.synchronize()));
    assert_eq!(adapter.opened(), 3);
    assert_eq!(adapter.dropped(), 3);
}

#[test]
fn admission_lease_retains_original_process_object_after_pid_reuse() {
    let pid = 100;
    let adapter = FakeProcessAdapter::default().with_process(
        pid,
        admission_script(
            10_000,
            Ok(Liveness::Alive),
            Ok(ProcessProtection::new(false, false)),
        ),
    );
    let authority = authority(adapter.clone());
    let original_target = target(pid, 10_000);
    let admission = authority
        .admit(&original_target, ProcessIntent::Restart)
        .unwrap();

    assert_eq!(adapter.opened(), 1);
    assert_eq!(adapter.dropped(), 0);
    let initial_events = adapter.handle_events();
    let [FakeHandleEvent::Opened {
        handle_id: original_handle_id,
        object_id: original_object_id,
        pid: opened_pid,
    }] = initial_events.as_slice()
    else {
        panic!("admission must retain exactly one original fake handle");
    };
    assert_eq!(*opened_pid, pid);

    let replacement_object_id = adapter.replace_process(
        pid,
        admission_script(
            10_001,
            Ok(Liveness::Alive),
            Ok(ProcessProtection::new(false, false)),
        ),
    );
    assert_ne!(replacement_object_id, *original_object_id);

    let replacement_handle = adapter
        .open_observation(pid, ObservationAccess::SNAPSHOT)
        .unwrap();
    assert_eq!(adapter.creation_time(&replacement_handle), Ok(10_001));
    assert_eq!(admission.target(), &original_target);
    assert_eq!(admission.intent(), ProcessIntent::Restart);
    assert_eq!(adapter.dropped(), 0);
    assert_eq!(
        adapter.handle_events(),
        vec![
            FakeHandleEvent::Opened {
                handle_id: *original_handle_id,
                object_id: *original_object_id,
                pid,
            },
            FakeHandleEvent::Opened {
                handle_id: 2,
                object_id: replacement_object_id,
                pid,
            },
        ]
    );

    drop(replacement_handle);
    assert_eq!(adapter.dropped(), 1);
    assert_eq!(
        adapter.handle_events(),
        vec![
            FakeHandleEvent::Opened {
                handle_id: *original_handle_id,
                object_id: *original_object_id,
                pid,
            },
            FakeHandleEvent::Opened {
                handle_id: 2,
                object_id: replacement_object_id,
                pid,
            },
            FakeHandleEvent::Dropped {
                handle_id: 2,
                object_id: replacement_object_id,
                pid,
            },
        ]
    );

    drop(admission);
    assert_eq!(adapter.opened(), 2);
    assert_eq!(adapter.dropped(), 2);
    assert_eq!(
        adapter.handle_events(),
        vec![
            FakeHandleEvent::Opened {
                handle_id: *original_handle_id,
                object_id: *original_object_id,
                pid,
            },
            FakeHandleEvent::Opened {
                handle_id: 2,
                object_id: replacement_object_id,
                pid,
            },
            FakeHandleEvent::Dropped {
                handle_id: 2,
                object_id: replacement_object_id,
                pid,
            },
            FakeHandleEvent::Dropped {
                handle_id: *original_handle_id,
                object_id: *original_object_id,
                pid,
            },
        ]
    );
    assert_eq!(
        adapter.calls(),
        vec![
            FakeCall::Open(pid),
            FakeCall::CreationTime(pid),
            FakeCall::Liveness(pid),
            FakeCall::Protection(pid),
            FakeCall::Open(pid),
            FakeCall::CreationTime(pid),
        ]
    );
}

#[test]
fn admission_source_keeps_the_lease_opaque_and_has_no_side_effect_adapter() {
    let process_module = include_str!("mod.rs");
    let process_core = include_str!("core.rs");

    assert_eq!(
        process_module
            .matches("core::CoreAdmission<windows::OwnedProcessHandle>")
            .count(),
        1
    );
    assert!(!process_module.contains("pub(crate) _inner"));
    assert!(!process_module.contains("raw_handle"));
    for forbidden in [
        "derive(Clone",
        "Serialize for CoreAdmission",
        "raw_handle",
        "open_operation",
        "terminate_process",
        "force_terminate",
        "restart_process",
    ] {
        let lease_source = process_core
            .split("struct CoreAdmission")
            .nth(1)
            .expect("CoreAdmission must be defined");
        assert!(!lease_source.contains(forbidden));
    }
    let adapter_source = process_core
        .split("trait ProcessPlatformAdapter")
        .nth(1)
        .expect("ProcessPlatformAdapter must be defined")
        .split("struct CoreAuthority")
        .next()
        .expect("CoreAuthority must follow the Adapter");
    for forbidden in ["terminate", "restart", "operation", "side_effect"] {
        assert!(!adapter_source.contains(forbidden));
    }
}

#[cfg(windows)]
mod windows_tests {
    use std::{
        cell::Cell,
        collections::VecDeque,
        ffi::OsString,
        os::windows::ffi::OsStringExt,
        process::{Child, Command, ExitStatus, Stdio},
        thread,
        time::{Duration, Instant},
    };

    use windows::{
        core::{Error as WindowsError, HRESULT},
        Win32::{
            Foundation::{
                ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, ERROR_NOT_FOUND, FILETIME, HANDLE,
                WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
            },
            System::Threading::{
                GetCurrentProcessId, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            },
        },
    };

    use super::*;
    use crate::process_control::windows::{
        classify_observation_error, classify_path_query_error, collect_candidates_with,
        filetime_to_u64, find_reenumerated_name, generate_host_instance_with,
        liveness_from_wait_result, open_observation_with, EnumerationStep, OwnedHandle,
        WindowsProcessAdapter,
    };
    use crate::process_control::{
        ProcessAuthority, ProcessAuthorityInitError, ProcessError, ProcessIntent,
    };

    const FIXTURE_TEST: &str = "process_control::tests::windows_tests::windows_fixture_child";
    const FIXTURE_MARKER: &str = "UIPILOT_PROCESS_CONTROL_TEST_CHILD";

    #[test]
    fn windows_cng_generation_seam_accepts_successful_fill() {
        let host = generate_host_instance_with(|bytes| {
            bytes.copy_from_slice(&[0x5a; 16]);
            Ok(())
        })
        .unwrap();

        assert_eq!(host.as_bytes(), [0x5a; 16]);
    }

    #[test]
    fn windows_cng_generation_seam_redacts_fill_failure() {
        assert_eq!(
            generate_host_instance_with(|_| Err(())),
            Err(ProcessAuthorityInitError::Unavailable)
        );
    }

    #[test]
    fn windows_toolhelp_end_is_distinct_from_traversal_failure() {
        let mut complete = VecDeque::from([
            EnumerationStep::Entry(candidate(41, "first.exe", Some(4))),
            EnumerationStep::End,
        ]);
        let complete = collect_candidates_with(|| complete.pop_front().unwrap());
        assert_eq!(complete, vec![Ok(candidate(41, "first.exe", Some(4)))]);

        let mut failed = VecDeque::from([
            EnumerationStep::Entry(candidate(42, "second.exe", None)),
            EnumerationStep::Failed,
        ]);
        let failed = collect_candidates_with(|| failed.pop_front().unwrap());
        assert_eq!(
            failed,
            vec![
                Ok(candidate(42, "second.exe", None)),
                Err(EnumerationFailure::Unavailable),
            ]
        );
    }

    #[test]
    fn windows_reenumeration_rejects_a_name_from_incomplete_traversal() {
        assert_eq!(
            find_reenumerated_name(
                vec![
                    Ok((41, "complete.exe".into())),
                    Ok((42, "other.exe".into()))
                ],
                41,
            ),
            Ok(Some("complete.exe".into()))
        );
        assert_eq!(
            find_reenumerated_name(
                vec![
                    Ok((41, "incomplete.exe".into())),
                    Err(EnumerationFailure::Unavailable),
                ],
                41,
            ),
            Err(ObservationFailure::IdentityUnavailable)
        );
    }

    #[test]
    fn windows_filetime_conversion_preserves_all_64_bits() {
        assert_eq!(
            filetime_to_u64(FILETIME {
                dwLowDateTime: 0x89ab_cdef,
                dwHighDateTime: 0x0123_4567,
            }),
            0x0123_4567_89ab_cdef
        );
    }

    #[test]
    fn windows_wait_result_mapping_is_exact() {
        assert_eq!(liveness_from_wait_result(WAIT_TIMEOUT), Ok(Liveness::Alive));
        assert_eq!(
            liveness_from_wait_result(WAIT_OBJECT_0),
            Ok(Liveness::Exited)
        );
        assert_eq!(
            liveness_from_wait_result(WAIT_FAILED),
            Err(ObservationFailure::AccessDenied)
        );
        assert_eq!(
            liveness_from_wait_result(windows::Win32::Foundation::WAIT_EVENT(17)),
            Err(ObservationFailure::AccessDenied)
        );
    }

    #[test]
    fn windows_unexpected_wait_result_is_a_snapshot_inspection_denial() {
        let pid = 7_000;
        let unexpected_wait_failure =
            liveness_from_wait_result(windows::Win32::Foundation::WAIT_EVENT(17)).unwrap_err();
        let adapter = FakeProcessAdapter::default()
            .with_enumeration(vec![Ok(candidate(pid, "wait.exe", None))])
            .with_process(
                pid,
                FakeProcessScript {
                    open: vec![Ok(())].into(),
                    creation_times: vec![Ok(70_000)].into(),
                    liveness: vec![Err(unexpected_wait_failure)].into(),
                    observations: vec![Ok(path_observation("wait.exe", r"C:\Apps\wait.exe"))]
                        .into(),
                    ..FakeProcessScript::default()
                },
            );

        let snapshot = authority(adapter.clone()).snapshot().unwrap();

        assert!(snapshot.processes().is_empty());
        assert_eq!(snapshot.omissions().inspection_denied(), 1);
        assert_eq!(snapshot.omissions().identity_unavailable(), 0);
        assert_eq!(snapshot.omissions().total(), 1);
        assert_eq!(adapter.opened(), 1);
        assert_eq!(adapter.dropped(), 1);
    }

    #[test]
    fn windows_open_observation_uses_exact_access_and_never_inherits() {
        let mut captured = None;

        let result = open_observation_with(
            7_001,
            ObservationAccess::SNAPSHOT,
            |access, inherit, pid| {
                captured = Some((access, inherit, pid));
                Err::<(), _>(ObservationFailure::AccessDenied)
            },
        );

        assert_eq!(result, Err(ObservationFailure::AccessDenied));
        assert_eq!(
            captured,
            Some((
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                7_001,
            ))
        );
    }

    #[test]
    fn windows_path_query_access_denied_enters_revalidated_pathless_fallback() {
        let pid = 7_002;
        let path_query_failure = classify_path_query_error(WindowsError::from_hresult(
            HRESULT::from_win32(ERROR_ACCESS_DENIED.0),
        ));
        let adapter = FakeProcessAdapter::default()
            .with_enumeration(vec![Ok(candidate(pid, "fallback.exe", Some(7)))])
            .with_process(
                pid,
                FakeProcessScript {
                    open: vec![Ok(())].into(),
                    creation_times: vec![Ok(70_020), Ok(70_020)].into(),
                    liveness: vec![Ok(Liveness::Alive), Ok(Liveness::Alive)].into(),
                    observations: vec![Err(path_query_failure)].into(),
                    reenumerated_names: vec![Ok(Some("fallback.exe".into()))].into(),
                    ..FakeProcessScript::default()
                },
            );

        let snapshot = authority(adapter.clone()).snapshot().unwrap();

        assert_eq!(snapshot.processes().len(), 1);
        assert_eq!(snapshot.processes()[0].name(), "fallback.exe");
        assert_eq!(snapshot.processes()[0].executable_path(), None);
        assert_eq!(snapshot.omissions().total(), 0);
        assert_eq!(
            adapter.calls(),
            vec![
                FakeCall::Enumerate,
                FakeCall::Open(pid),
                FakeCall::CreationTime(pid),
                FakeCall::Liveness(pid),
                FakeCall::Observation(pid),
                FakeCall::ReenumerateName(pid),
                FakeCall::ProcessNamesEquivalent,
                FakeCall::Liveness(pid),
                FakeCall::CreationTime(pid),
            ]
        );
        assert_eq!(adapter.opened(), 1);
        assert_eq!(adapter.dropped(), 1);
    }

    #[test]
    fn windows_path_query_error_classification_is_context_specific() {
        let error = |code| WindowsError::from_hresult(HRESULT::from_win32(code));

        assert_eq!(
            classify_path_query_error(error(ERROR_INVALID_PARAMETER.0)),
            ObservationFailure::ExitedOrChanged
        );
        assert_eq!(
            classify_path_query_error(error(ERROR_NOT_FOUND.0)),
            ObservationFailure::ExitedOrChanged
        );
        assert_eq!(
            classify_observation_error(error(ERROR_ACCESS_DENIED.0)),
            ObservationFailure::AccessDenied
        );
    }

    #[test]
    fn windows_process_names_use_ordinal_unicode_and_native_wide_values() {
        let adapter = WindowsProcessAdapter;
        assert!(adapter
            .process_names_equivalent(OsStr::new("ÖRBIT.EXE"), OsStr::new("örbit.exe"))
            .unwrap());

        let unpaired = OsString::from_wide(&[0xd800, b'.' as u16, b'e' as u16]);
        let same_unpaired = OsString::from_wide(&[0xd800, b'.' as u16, b'e' as u16]);
        let other_unpaired = OsString::from_wide(&[0xd801, b'.' as u16, b'e' as u16]);
        assert!(adapter
            .process_names_equivalent(&unpaired, &same_unpaired)
            .unwrap());
        assert!(!adapter
            .process_names_equivalent(&unpaired, &other_unpaired)
            .unwrap());
    }

    #[test]
    fn windows_current_process_path_reconciles_and_self_is_protected() {
        let authority = ProcessAuthority::new().unwrap();
        let current_pid = unsafe { GetCurrentProcessId() };
        let record = snapshot_record_for_pid(&authority, current_pid, Duration::from_secs(5));

        assert!(record.executable_path().is_some());
        assert_eq!(record.pid(), current_pid);
        assert_eq!(
            authority
                .admit(record.target(), ProcessIntent::Terminate)
                .err(),
            Some(ProcessError::ProtectedTarget)
        );
    }

    #[test]
    fn windows_current_process_changed_creation_time_is_target_changed() {
        let authority = ProcessAuthority::new().unwrap();
        let current_pid = unsafe { GetCurrentProcessId() };
        let record = snapshot_record_for_pid(&authority, current_pid, Duration::from_secs(5));
        let creation_time = record.target().creation_time();
        let changed_creation_time = if creation_time == u64::MAX {
            creation_time - 1
        } else {
            creation_time + 1
        };
        let changed_target = ProcessTarget::from_parts_for_test(
            record.target().host_instance_id().as_bytes(),
            record.pid(),
            changed_creation_time,
        );

        assert_eq!(
            authority
                .admit(&changed_target, ProcessIntent::Terminate)
                .err(),
            Some(ProcessError::TargetChanged)
        );
    }

    #[test]
    fn windows_current_process_critical_and_ppl_queries_are_readable_and_unprotected() {
        let adapter = WindowsProcessAdapter;
        let handle = adapter
            .open_observation(
                unsafe { GetCurrentProcessId() },
                ObservationAccess::SNAPSHOT,
            )
            .unwrap();

        assert_eq!(
            adapter.protection(&handle),
            Ok(ProcessProtection::new(false, false))
        );
    }

    #[test]
    fn windows_owned_handle_drop_invokes_injected_closer_exactly_once() {
        let close_calls = Cell::new(0);
        let handle = OwnedHandle::with_closer(HANDLE::default(), |_| {
            close_calls.set(close_calls.get() + 1);
        });

        assert_eq!(close_calls.get(), 0);
        drop(handle);
        assert_eq!(close_calls.get(), 1);
    }

    #[test]
    fn windows_retained_ordinary_child_can_be_admitted_read_only() {
        let mut child = FixtureChild::spawn(Duration::from_secs(30));
        let authority = ProcessAuthority::new().unwrap();
        let record = snapshot_record_for_pid(&authority, child.id(), Duration::from_secs(5));

        let admission = authority
            .admit(record.target(), ProcessIntent::Restart)
            .unwrap();
        drop(admission);

        child.cleanup();
    }

    #[test]
    fn windows_child_exit_code_259_is_target_exited_not_alive() {
        let mut child = FixtureChild::spawn(Duration::from_secs(2));
        let authority = ProcessAuthority::new().unwrap();
        let record = snapshot_record_for_pid(&authority, child.id(), Duration::from_secs(1));
        let admission = authority
            .admit(record.target(), ProcessIntent::ForceTerminate)
            .unwrap();
        drop(admission);

        let status = child
            .wait_for_exit(Duration::from_secs(5))
            .expect("fixture child did not exit in time");
        assert_eq!(status.code(), Some(259));
        assert_eq!(
            authority
                .admit(record.target(), ProcessIntent::ForceTerminate)
                .err(),
            Some(ProcessError::TargetExited)
        );
    }

    #[test]
    fn windows_inspectable_protected_target_is_rejected_or_environment_is_na() {
        let authority = ProcessAuthority::new().unwrap();
        let current_pid = unsafe { GetCurrentProcessId() };
        let snapshot = authority.snapshot().unwrap();

        if snapshot.processes().iter().any(|record| {
            record.pid() != current_pid
                && authority
                    .admit(record.target(), ProcessIntent::Terminate)
                    .err()
                    == Some(ProcessError::ProtectedTarget)
        }) {
            return;
        }

        eprintln!("environment N/A: no inspectable protected process was present");
    }

    #[test]
    fn windows_source_has_no_process_side_effect_or_fallback_path() {
        let source = include_str!("windows.rs");
        let module = include_str!("mod.rs");

        for forbidden in [
            "PowerShell",
            "powershell",
            "WMI",
            "wmic",
            "ShellExecute",
            "NtQuery",
            "SeDebugPrivilege",
            "GetExitCodeProcess",
            "TerminateProcess",
            "PROCESS_ALL_ACCESS",
            "PROCESS_TERMINATE",
        ] {
            assert!(!source.contains(forbidden), "found forbidden {forbidden}");
        }
        assert!(source.contains("BCryptGenRandom(None"));
        assert!(source.contains("BCRYPT_USE_SYSTEM_PREFERRED_RNG"));
        assert!(source.contains("OpenProcess(access, false, pid)"));
        assert!(module.contains("cfg(not(windows))"));
        assert!(!module.contains("raw_handle"));
    }

    #[test]
    #[ignore = "spawned explicitly by Windows process-control tests"]
    fn windows_fixture_child() {
        assert_eq!(std::env::var(FIXTURE_MARKER).as_deref(), Ok("1"));
        let delay = std::env::var("UIPILOT_PROCESS_CONTROL_TEST_DELAY_MS")
            .unwrap()
            .parse::<u64>()
            .unwrap();
        thread::sleep(Duration::from_millis(delay));
        std::process::exit(259);
    }

    fn snapshot_record_for_pid(
        authority: &ProcessAuthority,
        pid: u32,
        timeout: Duration,
    ) -> crate::process_control::ProcessRecord {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(record) = authority
                .snapshot()
                .unwrap()
                .processes()
                .iter()
                .find(|record| record.pid() == pid)
            {
                return record.clone();
            }
            assert!(
                Instant::now() < deadline,
                "process did not enter the catalog"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    struct FixtureChild {
        child: Child,
    }

    impl FixtureChild {
        fn spawn(delay: Duration) -> Self {
            let child = Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", FIXTURE_TEST, "--test-threads=1"])
                .env(FIXTURE_MARKER, "1")
                .env(
                    "UIPILOT_PROCESS_CONTROL_TEST_DELAY_MS",
                    delay.as_millis().to_string(),
                )
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            Self { child }
        }

        fn id(&self) -> u32 {
            self.child.id()
        }

        fn wait_for_exit(&mut self, timeout: Duration) -> Option<ExitStatus> {
            let deadline = Instant::now() + timeout;
            loop {
                match self.child.try_wait() {
                    Ok(Some(status)) => return Some(status),
                    Err(_) => return None,
                    Ok(None) => {}
                }
                if Instant::now() >= deadline {
                    return None;
                }
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn cleanup(&mut self) {
            assert!(
                self.cleanup_inner(),
                "fixture child cleanup did not complete"
            );
        }

        fn cleanup_inner(&mut self) -> bool {
            match self.child.try_wait() {
                Ok(Some(_)) => return true,
                Err(_) => return false,
                Ok(None) => {}
            }
            let _ = self.child.kill();
            self.wait_for_exit(Duration::from_secs(5)).is_some()
        }
    }

    impl Drop for FixtureChild {
        fn drop(&mut self) {
            let _ = self.cleanup_inner();
        }
    }
}
