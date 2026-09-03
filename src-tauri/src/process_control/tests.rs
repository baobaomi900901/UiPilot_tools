use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet, VecDeque},
    ffi::OsStr,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use super::core::{
    CoreAdmission, CoreAuthority, EnumerationFailure, HostInstanceId, Liveness, ObservationAccess,
    ObservationFailure, ProcessCandidate, ProcessError, ProcessIntent, ProcessObservation,
    ProcessPlatformAdapter, ProcessProtection, ProcessTarget,
};
use super::operation::{
    ExitWait, MonotonicClock, OperationAccess, OperationFailure, ProcessOperationAdapter,
    WindowCandidate, WindowFacts, WindowFailure,
};
use super::ProcessOperationError;

const HOST: [u8; 16] = [0x41; 16];
const SELF_PID: u32 = 9_999;
const SELF_CREATION_TIME: u64 = 99_990;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FakeCall {
    Enumerate,
    OpenObservation(u32),
    OpenOperation(u32),
    CreationTime(u32),
    Liveness(u32),
    Observation(u32),
    ReenumerateName(u32),
    ProcessNamesEquivalent,
    Protection(u32),
    EnumerateTopLevelWindows,
    WindowFacts(u64),
    PostClose(u64),
    TerminateProcess(u32),
    WaitForExit(u32, Duration),
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
    operation_open: VecDeque<Result<(), OperationFailure>>,
    creation_times: VecDeque<Result<u64, ObservationFailure>>,
    liveness: VecDeque<Result<Liveness, ObservationFailure>>,
    observations: VecDeque<Result<ProcessObservation, ObservationFailure>>,
    reenumerated_names: VecDeque<Result<Option<String>, ObservationFailure>>,
    protections: VecDeque<Result<ProcessProtection, ObservationFailure>>,
    terminations: VecDeque<Result<(), OperationFailure>>,
    waits: VecDeque<Result<ExitWait, OperationFailure>>,
}

struct FakeProcessObject {
    id: u64,
    script: FakeProcessScript,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FakeHandleKind {
    Observation,
    Operation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FakeHandleEvent {
    Opened {
        handle_id: u64,
        object_id: u64,
        pid: u32,
        kind: FakeHandleKind,
    },
    Dropped {
        handle_id: u64,
        object_id: u64,
        pid: u32,
        kind: FakeHandleKind,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum FakeEvent {
    Call {
        call: FakeCall,
        active_handles: Vec<u64>,
    },
    Handle(FakeHandleEvent),
    OperationClosure {
        operation_handle_id: u64,
        active_handles: Vec<u64>,
    },
}

struct FakeState {
    enumerations:
        VecDeque<Result<Vec<Result<ProcessCandidate, EnumerationFailure>>, EnumerationFailure>>,
    processes: BTreeMap<u32, Rc<RefCell<FakeProcessObject>>>,
    calls: Vec<FakeCall>,
    hooks: Vec<MutationHook>,
    requested_access: Vec<ObservationAccess>,
    requested_operation_access: Vec<OperationAccess>,
    process_name_equivalences: VecDeque<Result<bool, ObservationFailure>>,
    window_enumerations: VecDeque<Result<Vec<WindowCandidate<u64>>, OperationFailure>>,
    window_facts: BTreeMap<u64, VecDeque<Result<WindowFacts, WindowFailure>>>,
    post_close_outcomes: BTreeMap<u64, VecDeque<Result<(), WindowFailure>>>,
    post_close_calls: usize,
    terminate_calls: usize,
    terminate_exit_codes: Vec<u32>,
    opened: usize,
    next_handle_id: u64,
    next_object_id: u64,
    dropped: BTreeSet<u64>,
    active_handles: BTreeSet<u64>,
    handle_events: Vec<FakeHandleEvent>,
    events: Vec<FakeEvent>,
}

impl Default for FakeState {
    fn default() -> Self {
        Self {
            enumerations: VecDeque::new(),
            processes: BTreeMap::new(),
            calls: Vec::new(),
            hooks: Vec::new(),
            requested_access: Vec::new(),
            requested_operation_access: Vec::new(),
            process_name_equivalences: VecDeque::new(),
            window_enumerations: VecDeque::new(),
            window_facts: BTreeMap::new(),
            post_close_outcomes: BTreeMap::new(),
            post_close_calls: 0,
            terminate_calls: 0,
            terminate_exit_codes: Vec::new(),
            opened: 0,
            next_handle_id: 1,
            next_object_id: 1,
            dropped: BTreeSet::new(),
            active_handles: BTreeSet::new(),
            handle_events: Vec::new(),
            events: Vec::new(),
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
    kind: FakeHandleKind,
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
        assert!(state.active_handles.remove(&self.id));
        let event = FakeHandleEvent::Dropped {
            handle_id: self.id,
            object_id,
            pid: self.pid,
            kind: self.kind,
        };
        state.handle_events.push(event);
        state.events.push(FakeEvent::Handle(event));
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

    fn with_window_enumeration(
        self,
        value: Result<Vec<WindowCandidate<u64>>, OperationFailure>,
    ) -> Self {
        self.state.borrow_mut().window_enumerations.push_back(value);
        self
    }

    fn with_window_facts(
        self,
        window: u64,
        values: Vec<Result<WindowFacts, WindowFailure>>,
    ) -> Self {
        self.state
            .borrow_mut()
            .window_facts
            .insert(window, values.into());
        self
    }

    fn with_post_close_outcomes(self, window: u64, values: Vec<Result<(), WindowFailure>>) -> Self {
        self.state
            .borrow_mut()
            .post_close_outcomes
            .insert(window, values.into());
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

    fn requested_operation_access(&self) -> Vec<OperationAccess> {
        self.state.borrow().requested_operation_access.clone()
    }

    fn post_close_calls(&self) -> usize {
        self.state.borrow().post_close_calls
    }

    fn terminate_calls(&self) -> usize {
        self.state.borrow().terminate_calls
    }

    fn terminate_exit_codes(&self) -> Vec<u32> {
        self.state.borrow().terminate_exit_codes.clone()
    }

    fn events(&self) -> Vec<FakeEvent> {
        self.state.borrow().events.clone()
    }

    fn record_operation_closure(&self, handle: &FakeHandle) {
        let mut state = self.state.borrow_mut();
        let active_handles = state.active_handles.iter().copied().collect();
        state.events.push(FakeEvent::OperationClosure {
            operation_handle_id: handle.id,
            active_handles,
        });
    }

    fn finish_call(&self, call: FakeCall) {
        let mutations = {
            let mut state = self.state.borrow_mut();
            state.calls.push(call);
            let active_handles = state.active_handles.iter().copied().collect();
            state.events.push(FakeEvent::Call {
                call,
                active_handles,
            });
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

    fn make_handle(
        &self,
        pid: u32,
        object: Rc<RefCell<FakeProcessObject>>,
        kind: FakeHandleKind,
    ) -> FakeHandle {
        let object_id = object.borrow().id;
        let mut state = self.state.borrow_mut();
        let id = state.next_handle_id;
        state.next_handle_id += 1;
        state.opened += 1;
        assert!(state.active_handles.insert(id));
        let event = FakeHandleEvent::Opened {
            handle_id: id,
            object_id,
            pid,
            kind,
        };
        state.handle_events.push(event);
        state.events.push(FakeEvent::Handle(event));
        FakeHandle {
            id,
            pid,
            kind,
            object,
            state: Rc::clone(&self.state),
        }
    }

    fn pop_operation_value<T>(
        object: &Rc<RefCell<FakeProcessObject>>,
        select: impl FnOnce(&mut FakeProcessScript) -> &mut VecDeque<Result<T, OperationFailure>>,
    ) -> Result<T, OperationFailure> {
        select(&mut object.borrow_mut().script)
            .pop_front()
            .unwrap_or(Err(OperationFailure::Unavailable))
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
        let result = outcome.map(|()| self.make_handle(pid, object, FakeHandleKind::Observation));
        self.finish_call(FakeCall::OpenObservation(pid));
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

impl ProcessOperationAdapter for FakeProcessAdapter {
    type WindowToken = u64;

    fn open_operation(
        &self,
        pid: u32,
        access: OperationAccess,
    ) -> Result<Self::Handle, OperationFailure> {
        self.state
            .borrow_mut()
            .requested_operation_access
            .push(access);
        let object = self.current_process(pid);
        let outcome = Self::pop_operation_value(&object, |script| &mut script.operation_open);
        let result = outcome.map(|()| self.make_handle(pid, object, FakeHandleKind::Operation));
        self.finish_call(FakeCall::OpenOperation(pid));
        result
    }

    fn enumerate_top_level_windows(
        &self,
    ) -> Result<Vec<WindowCandidate<Self::WindowToken>>, OperationFailure> {
        let result = self
            .state
            .borrow_mut()
            .window_enumerations
            .pop_front()
            .unwrap_or(Err(OperationFailure::Unavailable));
        self.finish_call(FakeCall::EnumerateTopLevelWindows);
        result
    }

    fn window_facts(&self, window: Self::WindowToken) -> Result<WindowFacts, WindowFailure> {
        let result = self
            .state
            .borrow_mut()
            .window_facts
            .get_mut(&window)
            .and_then(VecDeque::pop_front)
            .unwrap_or(Err(WindowFailure::Unavailable));
        self.finish_call(FakeCall::WindowFacts(window));
        result
    }

    fn post_close(&self, window: Self::WindowToken) -> Result<(), WindowFailure> {
        let result = {
            let mut state = self.state.borrow_mut();
            state.post_close_calls += 1;
            state
                .post_close_outcomes
                .get_mut(&window)
                .and_then(VecDeque::pop_front)
                .unwrap_or(Err(WindowFailure::Unavailable))
        };
        self.finish_call(FakeCall::PostClose(window));
        result
    }

    fn terminate_process(
        &self,
        handle: &Self::Handle,
        exit_code: u32,
    ) -> Result<(), OperationFailure> {
        let result = {
            let mut state = self.state.borrow_mut();
            state.terminate_calls += 1;
            state.terminate_exit_codes.push(exit_code);
            drop(state);
            Self::pop_operation_value(&handle.object, |script| &mut script.terminations)
        };
        self.finish_call(FakeCall::TerminateProcess(handle.pid));
        result
    }

    fn wait_for_exit(
        &self,
        handle: &Self::Handle,
        timeout: Duration,
    ) -> Result<ExitWait, OperationFailure> {
        let result = Self::pop_operation_value(&handle.object, |script| &mut script.waits);
        self.finish_call(FakeCall::WaitForExit(handle.pid, timeout));
        result
    }
}

#[derive(Clone)]
struct FakeMonotonicClock {
    now: Rc<Cell<Instant>>,
    scripted: Rc<RefCell<VecDeque<Instant>>>,
    reads: Rc<Cell<usize>>,
}

impl FakeMonotonicClock {
    fn at(now: Instant) -> Self {
        Self {
            now: Rc::new(Cell::new(now)),
            scripted: Rc::new(RefCell::new(VecDeque::new())),
            reads: Rc::new(Cell::new(0)),
        }
    }

    fn with_readings(base: Instant, offsets: Vec<Duration>) -> Self {
        let clock = Self::at(base);
        clock.scripted.borrow_mut().extend(
            offsets
                .into_iter()
                .map(|offset| base.checked_add(offset).unwrap()),
        );
        clock
    }

    fn advance(&self, duration: Duration) {
        self.now.set(self.now.get() + duration);
    }

    fn reads(&self) -> usize {
        self.reads.get()
    }
}

impl MonotonicClock for FakeMonotonicClock {
    fn now(&self) -> Instant {
        self.reads.set(self.reads.get() + 1);
        if let Some(now) = self.scripted.borrow_mut().pop_front() {
            self.now.set(now);
        }
        self.now.get()
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

fn final_admission_script(
    creation_time: u64,
    operation_open: Result<(), OperationFailure>,
    operation_creation_time: Result<u64, ObservationFailure>,
    operation_liveness: Result<Liveness, ObservationFailure>,
    operation_protection: Result<ProcessProtection, ObservationFailure>,
) -> FakeProcessScript {
    FakeProcessScript {
        open: vec![Ok(())].into(),
        operation_open: vec![operation_open].into(),
        creation_times: vec![Ok(creation_time), operation_creation_time].into(),
        liveness: vec![Ok(Liveness::Alive), operation_liveness].into(),
        protections: vec![
            Ok(ProcessProtection::new(false, false)),
            operation_protection,
        ]
        .into(),
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
            FakeCall::OpenObservation(11),
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
        vec![FakeCall::OpenObservation(pid), FakeCall::CreationTime(pid),]
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
    assert_eq!(adapter.calls(), vec![FakeCall::OpenObservation(pid)]);
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
        assert_eq!(adapter.calls(), vec![FakeCall::OpenObservation(pid)]);
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
            vec![FakeCall::OpenObservation(pid), FakeCall::CreationTime(pid),]
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
            FakeCall::OpenObservation(pid),
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
                FakeCall::OpenObservation(pid),
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
            FakeCall::OpenObservation(pid),
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
            FakeCall::OpenObservation(SELF_PID),
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
        kind: FakeHandleKind::Observation,
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
                kind: FakeHandleKind::Observation,
            },
            FakeHandleEvent::Opened {
                handle_id: 2,
                object_id: replacement_object_id,
                pid,
                kind: FakeHandleKind::Observation,
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
                kind: FakeHandleKind::Observation,
            },
            FakeHandleEvent::Opened {
                handle_id: 2,
                object_id: replacement_object_id,
                pid,
                kind: FakeHandleKind::Observation,
            },
            FakeHandleEvent::Dropped {
                handle_id: 2,
                object_id: replacement_object_id,
                pid,
                kind: FakeHandleKind::Observation,
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
                kind: FakeHandleKind::Observation,
            },
            FakeHandleEvent::Opened {
                handle_id: 2,
                object_id: replacement_object_id,
                pid,
                kind: FakeHandleKind::Observation,
            },
            FakeHandleEvent::Dropped {
                handle_id: 2,
                object_id: replacement_object_id,
                pid,
                kind: FakeHandleKind::Observation,
            },
            FakeHandleEvent::Dropped {
                handle_id: *original_handle_id,
                object_id: *original_object_id,
                pid,
                kind: FakeHandleKind::Observation,
            },
        ]
    );
    assert_eq!(
        adapter.calls(),
        vec![
            FakeCall::OpenObservation(pid),
            FakeCall::CreationTime(pid),
            FakeCall::Liveness(pid),
            FakeCall::Protection(pid),
            FakeCall::OpenObservation(pid),
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

mod operation_admission {
    use super::*;

    fn successful_final_script(creation_time: u64) -> FakeProcessScript {
        final_admission_script(
            creation_time,
            Ok(()),
            Ok(creation_time),
            Ok(Liveness::Alive),
            Ok(ProcessProtection::new(false, false)),
        )
    }

    #[test]
    fn operation_error_display_is_exact_and_redacted() {
        let cases = [
            (ProcessOperationError::IntentMismatch, "IntentMismatch"),
            (ProcessOperationError::TargetExited, "TargetExited"),
            (ProcessOperationError::TargetChanged, "TargetChanged"),
            (ProcessOperationError::InspectionDenied, "InspectionDenied"),
            (ProcessOperationError::ProtectedTarget, "ProtectedTarget"),
            (
                ProcessOperationError::OperationAccessDenied,
                "OperationAccessDenied",
            ),
            (
                ProcessOperationError::GracefulCloseUnavailable,
                "GracefulCloseUnavailable",
            ),
            (
                ProcessOperationError::GracefulCloseTimedOut,
                "GracefulCloseTimedOut",
            ),
            (
                ProcessOperationError::OperationUnavailable,
                "OperationUnavailable",
            ),
            (
                ProcessOperationError::TerminationPending,
                "TerminationPending",
            ),
            (
                ProcessOperationError::CompletionUnknown,
                "CompletionUnknown",
            ),
            (
                ProcessOperationError::PlatformUnsupported,
                "PlatformUnsupported",
            ),
        ];

        for (error, code) in cases {
            let display = error.to_string();
            assert_eq!(display, code);
            for sensitive in ["424242", "sensitive.exe", "raw-win32-5"] {
                assert!(!display.contains(sensitive));
            }
        }
    }

    #[test]
    fn private_contract_values_and_fake_seams_are_exact_and_scriptable() {
        assert!(OperationAccess::GRACEFUL.query_limited_information());
        assert!(OperationAccess::GRACEFUL.synchronize());
        assert!(!OperationAccess::GRACEFUL.terminate());
        assert!(OperationAccess::FORCE.query_limited_information());
        assert!(OperationAccess::FORCE.synchronize());
        assert!(OperationAccess::FORCE.terminate());

        let candidate = WindowCandidate::new(7_u64, 41, true, false);
        assert_eq!(candidate.token(), 7);
        assert_eq!(candidate.pid(), 41);
        assert!(candidate.visible());
        assert!(!candidate.has_owner());
        let facts = WindowFacts::new(41, true, false);
        assert_eq!(facts.pid(), 41);
        assert!(facts.visible());
        assert!(!facts.has_owner());

        let pid = 41;
        let adapter = FakeProcessAdapter::default()
            .with_process(
                pid,
                FakeProcessScript {
                    operation_open: vec![Ok(())].into(),
                    terminations: vec![Ok(())].into(),
                    waits: vec![Ok(ExitWait::Exited)].into(),
                    ..FakeProcessScript::default()
                },
            )
            .with_window_enumeration(Ok(vec![candidate]))
            .with_window_facts(7, vec![Ok(facts)])
            .with_post_close_outcomes(7, vec![Ok(())]);
        let handle = adapter.open_operation(pid, OperationAccess::FORCE).unwrap();
        assert_eq!(adapter.enumerate_top_level_windows(), Ok(vec![candidate]));
        assert_eq!(adapter.window_facts(7), Ok(facts));
        assert_eq!(adapter.post_close(7), Ok(()));
        assert_eq!(adapter.terminate_process(&handle, 1), Ok(()));
        assert_eq!(
            adapter.wait_for_exit(&handle, Duration::from_millis(25)),
            Ok(ExitWait::Exited)
        );
        assert_eq!(adapter.post_close_calls(), 1);
        assert_eq!(adapter.terminate_calls(), 1);
        drop(handle);

        let base = Instant::now();
        let clock = FakeMonotonicClock::at(base);
        assert_eq!(clock.now(), base);
        clock.advance(Duration::from_millis(25));
        assert_eq!(clock.now(), base + Duration::from_millis(25));
    }

    #[test]
    fn intent_mismatch_drops_the_lease_before_any_operation_call() {
        let pid = 42;
        let creation_time = 4_200;
        let adapter =
            FakeProcessAdapter::default().with_process(pid, successful_final_script(creation_time));
        let authority = authority(adapter.clone());
        let admission = authority
            .admit(&target(pid, creation_time), ProcessIntent::Terminate)
            .unwrap();
        let closure_calls = Cell::new(0);

        let result = authority.with_final_operation_admission(
            admission,
            ProcessIntent::ForceTerminate,
            |_adapter, _handle| {
                closure_calls.set(closure_calls.get() + 1);
                Ok(())
            },
        );

        assert_eq!(result, Err(ProcessOperationError::IntentMismatch));
        assert_eq!(closure_calls.get(), 0);
        assert!(adapter.requested_operation_access().is_empty());
        assert_eq!(adapter.post_close_calls(), 0);
        assert_eq!(adapter.terminate_calls(), 0);
        assert_eq!(adapter.opened(), 1);
        assert_eq!(adapter.dropped(), 1);
        assert_eq!(
            adapter.calls(),
            vec![
                FakeCall::OpenObservation(pid),
                FakeCall::CreationTime(pid),
                FakeCall::Liveness(pid),
                FakeCall::Protection(pid),
            ]
        );
    }

    #[test]
    fn supported_intents_select_only_their_exact_operation_access() {
        for (intent, expected_access) in [
            (ProcessIntent::Terminate, OperationAccess::GRACEFUL),
            (ProcessIntent::ForceTerminate, OperationAccess::FORCE),
        ] {
            let pid = 43;
            let creation_time = 4_300;
            let adapter = FakeProcessAdapter::default()
                .with_process(pid, successful_final_script(creation_time));
            let authority = authority(adapter.clone());
            let admission = authority
                .admit(&target(pid, creation_time), intent)
                .unwrap();
            let closure_calls = Cell::new(0);

            let result =
                authority.with_final_operation_admission(admission, intent, |adapter, handle| {
                    closure_calls.set(closure_calls.get() + 1);
                    adapter.record_operation_closure(handle);
                    Ok(())
                });

            assert_eq!(result, Ok(()));
            assert_eq!(closure_calls.get(), 1);
            assert_eq!(adapter.requested_operation_access(), vec![expected_access]);
            assert_eq!(adapter.opened(), 2);
            assert_eq!(adapter.dropped(), 2);
        }
    }

    #[test]
    fn operation_open_failures_have_stable_classifications_and_no_closure() {
        for (failure, expected) in [
            (
                OperationFailure::ExitedOrChanged,
                ProcessOperationError::TargetExited,
            ),
            (
                OperationFailure::AccessDenied,
                ProcessOperationError::OperationAccessDenied,
            ),
            (
                OperationFailure::Unavailable,
                ProcessOperationError::OperationUnavailable,
            ),
        ] {
            let pid = 44;
            let creation_time = 4_400;
            let adapter = FakeProcessAdapter::default().with_process(
                pid,
                final_admission_script(
                    creation_time,
                    Err(failure),
                    Ok(creation_time),
                    Ok(Liveness::Alive),
                    Ok(ProcessProtection::new(false, false)),
                ),
            );
            let authority = authority(adapter.clone());
            let admission = authority
                .admit(&target(pid, creation_time), ProcessIntent::Terminate)
                .unwrap();
            let closure_calls = Cell::new(0);

            let result = authority.with_final_operation_admission(
                admission,
                ProcessIntent::Terminate,
                |_adapter, _handle| {
                    closure_calls.set(closure_calls.get() + 1);
                    Ok(())
                },
            );

            assert_eq!(result, Err(expected));
            assert_eq!(closure_calls.get(), 0);
            assert_eq!(adapter.opened(), 1);
            assert_eq!(adapter.dropped(), 1);
            assert_eq!(
                adapter.requested_operation_access(),
                vec![OperationAccess::GRACEFUL]
            );
            assert_eq!(adapter.calls().last(), Some(&FakeCall::OpenOperation(pid)));
        }
    }

    #[test]
    fn replacement_creation_time_stops_before_replacement_liveness_or_protection() {
        let pid = 45;
        let creation_time = 4_500;
        let adapter = FakeProcessAdapter::default().with_process(
            pid,
            admission_script(
                creation_time,
                Ok(Liveness::Alive),
                Ok(ProcessProtection::new(false, false)),
            ),
        );
        let authority = authority(adapter.clone());
        let admission = authority
            .admit(&target(pid, creation_time), ProcessIntent::Terminate)
            .unwrap();
        adapter.replace_process(
            pid,
            FakeProcessScript {
                operation_open: vec![Ok(())].into(),
                creation_times: vec![Ok(4_501)].into(),
                liveness: vec![Ok(Liveness::Alive)].into(),
                protections: vec![Err(ObservationFailure::AccessDenied)].into(),
                ..FakeProcessScript::default()
            },
        );
        let closure_calls = Cell::new(0);

        let result = authority.with_final_operation_admission(
            admission,
            ProcessIntent::Terminate,
            |_adapter, _handle| {
                closure_calls.set(closure_calls.get() + 1);
                Ok(())
            },
        );

        assert_eq!(result, Err(ProcessOperationError::TargetChanged));
        let display = result.unwrap_err().to_string();
        assert_eq!(display, "TargetChanged");
        assert!(!display.contains("4501"));
        assert_eq!(closure_calls.get(), 0);
        assert_eq!(adapter.opened(), 2);
        assert_eq!(adapter.dropped(), 2);
        assert_eq!(
            &adapter.calls()[4..],
            [FakeCall::OpenOperation(pid), FakeCall::CreationTime(pid),]
        );
    }

    #[test]
    fn final_creation_and_liveness_failures_preserve_exit_then_inspection_precedence() {
        for (creation_result, expected) in [
            (
                Err(ObservationFailure::ExitedOrChanged),
                ProcessOperationError::TargetExited,
            ),
            (
                Err(ObservationFailure::AccessDenied),
                ProcessOperationError::InspectionDenied,
            ),
            (
                Err(ObservationFailure::IdentityUnavailable),
                ProcessOperationError::InspectionDenied,
            ),
        ] {
            let pid = 46;
            let creation_time = 4_600;
            let adapter = FakeProcessAdapter::default().with_process(
                pid,
                final_admission_script(
                    creation_time,
                    Ok(()),
                    creation_result,
                    Ok(Liveness::Alive),
                    Ok(ProcessProtection::new(false, false)),
                ),
            );
            let authority = authority(adapter.clone());
            let admission = authority
                .admit(&target(pid, creation_time), ProcessIntent::Terminate)
                .unwrap();
            let closure_calls = Cell::new(0);

            let result = authority.with_final_operation_admission(
                admission,
                ProcessIntent::Terminate,
                |_adapter, _handle| {
                    closure_calls.set(closure_calls.get() + 1);
                    Ok(())
                },
            );

            assert_eq!(result, Err(expected));
            assert_eq!(closure_calls.get(), 0);
            assert_eq!(adapter.opened(), 2);
            assert_eq!(adapter.dropped(), 2);
            assert_eq!(
                adapter
                    .calls()
                    .iter()
                    .filter(|call| matches!(call, FakeCall::Liveness(_)))
                    .count(),
                1
            );
            assert_eq!(
                adapter
                    .calls()
                    .iter()
                    .filter(|call| matches!(call, FakeCall::Protection(_)))
                    .count(),
                1
            );
        }

        for (liveness, expected) in [
            (Ok(Liveness::Exited), ProcessOperationError::TargetExited),
            (
                Err(ObservationFailure::ExitedOrChanged),
                ProcessOperationError::InspectionDenied,
            ),
            (
                Err(ObservationFailure::AccessDenied),
                ProcessOperationError::InspectionDenied,
            ),
        ] {
            let pid = 47;
            let creation_time = 4_700;
            let adapter = FakeProcessAdapter::default().with_process(
                pid,
                final_admission_script(
                    creation_time,
                    Ok(()),
                    Ok(creation_time),
                    liveness,
                    Err(ObservationFailure::IdentityUnavailable),
                ),
            );
            let authority = authority(adapter.clone());
            let admission = authority
                .admit(&target(pid, creation_time), ProcessIntent::Terminate)
                .unwrap();
            let closure_calls = Cell::new(0);

            let result = authority.with_final_operation_admission(
                admission,
                ProcessIntent::Terminate,
                |_adapter, _handle| {
                    closure_calls.set(closure_calls.get() + 1);
                    Ok(())
                },
            );

            assert_eq!(result, Err(expected));
            assert_eq!(closure_calls.get(), 0);
            assert_eq!(adapter.opened(), 2);
            assert_eq!(adapter.dropped(), 2);
            assert_eq!(
                adapter
                    .calls()
                    .iter()
                    .filter(|call| matches!(call, FakeCall::Protection(_)))
                    .count(),
                1
            );
        }
    }

    #[test]
    fn unreadable_protection_precedes_self_and_every_protected_class_fails_closed() {
        let pid = 48;
        let creation_time = 4_800;
        let adapter = FakeProcessAdapter::default().with_process(
            pid,
            final_admission_script(
                creation_time,
                Ok(()),
                Ok(creation_time),
                Ok(Liveness::Alive),
                Err(ObservationFailure::IdentityUnavailable),
            ),
        );
        let admitting_authority = authority(adapter.clone());
        let admission = admitting_authority
            .admit(&target(pid, creation_time), ProcessIntent::Terminate)
            .unwrap();
        let self_authority = authority_with_self(adapter.clone(), pid, creation_time);
        let closure_calls = Cell::new(0);
        let result = self_authority.with_final_operation_admission(
            admission,
            ProcessIntent::Terminate,
            |_adapter, _handle| {
                closure_calls.set(closure_calls.get() + 1);
                Ok(())
            },
        );
        assert_eq!(result, Err(ProcessOperationError::InspectionDenied));
        assert_eq!(closure_calls.get(), 0);
        assert_eq!(adapter.opened(), 2);
        assert_eq!(adapter.dropped(), 2);

        for (pid, protection, exact_self) in [
            (49, ProcessProtection::new(false, false), true),
            (50, ProcessProtection::new(true, false), false),
            (51, ProcessProtection::new(false, true), false),
        ] {
            let creation_time = u64::from(pid) * 100;
            let adapter = FakeProcessAdapter::default().with_process(
                pid,
                final_admission_script(
                    creation_time,
                    Ok(()),
                    Ok(creation_time),
                    Ok(Liveness::Alive),
                    Ok(protection),
                ),
            );
            let admitting_authority = authority(adapter.clone());
            let admission = admitting_authority
                .admit(&target(pid, creation_time), ProcessIntent::Terminate)
                .unwrap();
            let operation_authority = if exact_self {
                authority_with_self(adapter.clone(), pid, creation_time)
            } else {
                authority(adapter.clone())
            };
            let closure_calls = Cell::new(0);

            let result = operation_authority.with_final_operation_admission(
                admission,
                ProcessIntent::Terminate,
                |_adapter, _handle| {
                    closure_calls.set(closure_calls.get() + 1);
                    Ok(())
                },
            );

            assert_eq!(result, Err(ProcessOperationError::ProtectedTarget));
            assert_eq!(closure_calls.get(), 0);
            assert_eq!(adapter.opened(), 2);
            assert_eq!(adapter.dropped(), 2);
        }
    }

    #[test]
    fn handle_timeline_keeps_both_handles_through_checks_then_only_operation_in_closure() {
        let pid = 52;
        let creation_time = 5_200;
        let adapter =
            FakeProcessAdapter::default().with_process(pid, successful_final_script(creation_time));
        let authority = authority(adapter.clone());
        let admission = authority
            .admit(&target(pid, creation_time), ProcessIntent::Terminate)
            .unwrap();

        let result = authority.with_final_operation_admission(
            admission,
            ProcessIntent::Terminate,
            |adapter, handle| {
                adapter.record_operation_closure(handle);
                Ok(())
            },
        );

        assert_eq!(result, Ok(()));
        let events = adapter.events();
        let final_creation = events
            .iter()
            .rposition(|event| {
                matches!(
                    event,
                    FakeEvent::Call {
                        call: FakeCall::CreationTime(52),
                        active_handles,
                    } if active_handles == &[1, 2]
                )
            })
            .expect("final creation check must retain both handles");
        let final_liveness = events
            .iter()
            .rposition(|event| {
                matches!(
                    event,
                    FakeEvent::Call {
                        call: FakeCall::Liveness(52),
                        active_handles,
                    } if active_handles == &[1, 2]
                )
            })
            .expect("final liveness check must retain both handles");
        let final_protection = events
            .iter()
            .rposition(|event| {
                matches!(
                    event,
                    FakeEvent::Call {
                        call: FakeCall::Protection(52),
                        active_handles,
                    } if active_handles == &[1, 2]
                )
            })
            .expect("final protection check must retain both handles");
        let admission_drop = events
            .iter()
            .position(|event| {
                matches!(
                    event,
                    FakeEvent::Handle(FakeHandleEvent::Dropped {
                        handle_id: 1,
                        kind: FakeHandleKind::Observation,
                        ..
                    })
                )
            })
            .expect("admission handle must drop");
        let closure = events
            .iter()
            .position(|event| {
                matches!(
                    event,
                    FakeEvent::OperationClosure {
                        operation_handle_id: 2,
                        active_handles,
                    } if active_handles == &[2]
                )
            })
            .expect("closure must retain only the operation handle");
        let operation_drop = events
            .iter()
            .position(|event| {
                matches!(
                    event,
                    FakeEvent::Handle(FakeHandleEvent::Dropped {
                        handle_id: 2,
                        kind: FakeHandleKind::Operation,
                        ..
                    })
                )
            })
            .expect("operation handle must drop after the closure");

        assert!(final_creation < final_liveness);
        assert!(final_liveness < final_protection);
        assert!(final_protection < admission_drop);
        assert!(admission_drop < closure);
        assert!(closure < operation_drop);
        assert_eq!(adapter.opened(), 2);
        assert_eq!(adapter.dropped(), 2);
    }

    #[test]
    fn operation_contract_isolated_from_stage_one_core_and_module_exports() {
        let process_core = include_str!("core.rs");
        let process_module = include_str!("mod.rs");

        for forbidden in [
            "OperationAccess",
            "OperationFailure",
            "ProcessOperationAdapter",
            "open_operation",
            "WindowToken",
            "HANDLE",
            "HWND",
        ] {
            assert!(
                !process_core.contains(forbidden),
                "core contains {forbidden}"
            );
        }
        assert!(process_module.contains("mod operation;"));
        assert!(process_module
            .contains("pub(crate) type ProcessOperationError = operation::ProcessOperationError;"));
        for private_contract in [
            "OperationAccess",
            "OperationFailure",
            "WindowFailure",
            "WindowCandidate",
            "WindowFacts",
            "ExitWait",
            "ProcessOperationAdapter",
            "MonotonicClock",
        ] {
            assert!(!process_module.contains(&format!("pub(crate) type {private_contract}")));
        }
    }
}

mod operation_terminate {
    use super::*;

    fn graceful_script(
        creation_time: u64,
        waits: Vec<Result<ExitWait, OperationFailure>>,
    ) -> FakeProcessScript {
        FakeProcessScript {
            open: vec![Ok(())].into(),
            operation_open: vec![Ok(())].into(),
            creation_times: vec![Ok(creation_time), Ok(creation_time)].into(),
            liveness: vec![Ok(Liveness::Alive), Ok(Liveness::Alive)].into(),
            protections: vec![
                Ok(ProcessProtection::new(false, false)),
                Ok(ProcessProtection::new(false, false)),
            ]
            .into(),
            waits: waits.into(),
            ..FakeProcessScript::default()
        }
    }

    fn terminate(
        adapter: FakeProcessAdapter,
        pid: u32,
        creation_time: u64,
        clock: &FakeMonotonicClock,
    ) -> Result<(), ProcessOperationError> {
        let authority = authority(adapter);
        let admission = authority
            .admit(&target(pid, creation_time), ProcessIntent::Terminate)
            .unwrap();
        authority.terminate_with_clock(admission, clock)
    }

    #[test]
    fn filters_enumeration_facts_and_posts_each_valid_window_once_in_order() {
        let pid = 60;
        let creation_time = 6_000;
        let adapter = FakeProcessAdapter::default()
            .with_process(
                pid,
                graceful_script(
                    creation_time,
                    vec![
                        Ok(ExitWait::TimedOut),
                        Ok(ExitWait::TimedOut),
                        Ok(ExitWait::TimedOut),
                        Ok(ExitWait::Exited),
                        Ok(ExitWait::Exited),
                    ],
                ),
            )
            .with_window_enumeration(Ok(vec![
                WindowCandidate::new(1, pid, true, false),
                WindowCandidate::new(2, pid + 1, true, false),
                WindowCandidate::new(3, pid, false, false),
                WindowCandidate::new(4, pid, true, true),
                WindowCandidate::new(5, pid, true, false),
            ]))
            .with_window_facts(1, vec![Ok(WindowFacts::new(pid, true, false))])
            .with_window_facts(5, vec![Ok(WindowFacts::new(pid, true, false))])
            .with_post_close_outcomes(1, vec![Ok(())])
            .with_post_close_outcomes(5, vec![Ok(())]);
        let clock = FakeMonotonicClock::at(Instant::now());

        let result = terminate(adapter.clone(), pid, creation_time, &clock);

        assert_eq!(result, Ok(()));
        assert_eq!(adapter.post_close_calls(), 2);
        assert_eq!(adapter.terminate_calls(), 0);
        assert_eq!(
            adapter.requested_operation_access(),
            vec![OperationAccess::GRACEFUL]
        );
        assert_eq!(
            adapter
                .calls()
                .into_iter()
                .filter(|call| matches!(call, FakeCall::WindowFacts(_) | FakeCall::PostClose(_)))
                .collect::<Vec<_>>(),
            vec![
                FakeCall::WindowFacts(1),
                FakeCall::PostClose(1),
                FakeCall::WindowFacts(5),
                FakeCall::PostClose(5),
            ]
        );
    }

    #[test]
    fn adapter_candidate_contract_has_no_child_or_message_only_classification() {
        let source = include_str!("operation.rs");
        let candidate_contract = source
            .split("pub(super) struct WindowCandidate")
            .nth(1)
            .unwrap()
            .split("impl<W: Copy>")
            .next()
            .unwrap();
        let adapter_contract = source
            .split("pub(super) trait ProcessOperationAdapter")
            .nth(1)
            .unwrap()
            .split("pub(super) trait MonotonicClock")
            .next()
            .unwrap();

        for unsupported_candidate_class in ["child", "message_only", "message-only"] {
            assert!(!candidate_contract.contains(unsupported_candidate_class));
            assert!(!adapter_contract.contains(unsupported_candidate_class));
        }
        assert!(adapter_contract.contains("enumerate_top_level_windows"));
    }

    #[test]
    fn no_initial_candidate_is_unavailable_unless_the_process_has_exited() {
        for (poll, expected) in [
            (
                ExitWait::TimedOut,
                Err(ProcessOperationError::GracefulCloseUnavailable),
            ),
            (ExitWait::Exited, Ok(())),
        ] {
            let pid = 61;
            let creation_time = 6_100;
            let adapter = FakeProcessAdapter::default()
                .with_process(pid, graceful_script(creation_time, vec![Ok(poll)]))
                .with_window_enumeration(Ok(vec![
                    WindowCandidate::new(1, pid + 1, true, false),
                    WindowCandidate::new(2, pid, false, false),
                    WindowCandidate::new(3, pid, true, true),
                ]));
            let clock = FakeMonotonicClock::at(Instant::now());

            let result = terminate(adapter.clone(), pid, creation_time, &clock);

            assert_eq!(result, expected);
            assert_eq!(adapter.post_close_calls(), 0);
            assert_eq!(adapter.terminate_calls(), 0);
            assert_eq!(
                adapter
                    .calls()
                    .iter()
                    .filter(|call| matches!(call, FakeCall::EnumerateTopLevelWindows))
                    .count(),
                1
            );
            assert_eq!(
                adapter.calls().last(),
                Some(&FakeCall::WaitForExit(pid, Duration::ZERO))
            );
        }
    }

    #[test]
    fn changed_and_gone_candidates_are_skipped_without_retargeting() {
        for (final_poll, expected) in [
            (
                ExitWait::TimedOut,
                Err(ProcessOperationError::GracefulCloseUnavailable),
            ),
            (ExitWait::Exited, Ok(())),
        ] {
            let pid = 62;
            let creation_time = 6_200;
            let adapter = FakeProcessAdapter::default()
                .with_process(
                    pid,
                    graceful_script(
                        creation_time,
                        vec![
                            Ok(ExitWait::TimedOut),
                            Ok(ExitWait::TimedOut),
                            Ok(ExitWait::TimedOut),
                            Ok(ExitWait::TimedOut),
                            Ok(ExitWait::TimedOut),
                            Ok(final_poll),
                        ],
                    ),
                )
                .with_window_enumeration(Ok(vec![
                    WindowCandidate::new(10, pid, true, false),
                    WindowCandidate::new(11, pid, true, false),
                    WindowCandidate::new(12, pid, true, false),
                    WindowCandidate::new(13, pid, true, false),
                    WindowCandidate::new(14, pid, true, false),
                ]))
                .with_window_facts(10, vec![Ok(WindowFacts::new(pid + 1, true, false))])
                .with_window_facts(11, vec![Ok(WindowFacts::new(pid, false, false))])
                .with_window_facts(12, vec![Ok(WindowFacts::new(pid, true, true))])
                .with_window_facts(13, vec![Err(WindowFailure::Gone)])
                .with_window_facts(14, vec![Ok(WindowFacts::new(pid, true, false))])
                .with_post_close_outcomes(14, vec![Err(WindowFailure::Gone)]);
            let clock = FakeMonotonicClock::at(Instant::now());

            let result = terminate(adapter.clone(), pid, creation_time, &clock);

            assert_eq!(result, expected);
            assert_eq!(adapter.post_close_calls(), 1);
            assert_eq!(adapter.terminate_calls(), 0);
            assert_eq!(
                adapter
                    .calls()
                    .iter()
                    .filter(|call| matches!(call, FakeCall::EnumerateTopLevelWindows))
                    .count(),
                1
            );
            assert!(!adapter.calls().iter().any(|call| {
                matches!(
                    call,
                    FakeCall::PostClose(10)
                        | FakeCall::PostClose(11)
                        | FakeCall::PostClose(12)
                        | FakeCall::PostClose(13)
                )
            }));
        }
    }

    #[test]
    fn exit_before_the_first_or_between_posts_stops_side_effects_and_succeeds() {
        let pid = 63;
        let creation_time = 6_300;
        let before = FakeProcessAdapter::default()
            .with_process(
                pid,
                graceful_script(creation_time, vec![Ok(ExitWait::Exited)]),
            )
            .with_window_enumeration(Ok(vec![WindowCandidate::new(1, pid, true, false)]));
        let clock = FakeMonotonicClock::at(Instant::now());

        assert_eq!(
            terminate(before.clone(), pid, creation_time, &clock),
            Ok(())
        );
        assert_eq!(before.post_close_calls(), 0);
        assert!(!before
            .calls()
            .iter()
            .any(|call| matches!(call, FakeCall::WindowFacts(_))));

        let between = FakeProcessAdapter::default()
            .with_process(
                pid,
                graceful_script(
                    creation_time,
                    vec![Ok(ExitWait::TimedOut), Ok(ExitWait::Exited)],
                ),
            )
            .with_window_enumeration(Ok(vec![
                WindowCandidate::new(1, pid, true, false),
                WindowCandidate::new(2, pid, true, false),
            ]))
            .with_window_facts(1, vec![Ok(WindowFacts::new(pid, true, false))])
            .with_post_close_outcomes(1, vec![Ok(())]);

        assert_eq!(
            terminate(between.clone(), pid, creation_time, &clock),
            Ok(())
        );
        assert_eq!(between.post_close_calls(), 1);
        assert!(!between
            .calls()
            .iter()
            .any(|call| { matches!(call, FakeCall::WindowFacts(2) | FakeCall::PostClose(2)) }));
        assert_eq!(before.terminate_calls() + between.terminate_calls(), 0);
    }

    #[test]
    fn all_denied_failures_are_distinct_from_mixed_or_unavailable_failures() {
        let pid = 64;
        let creation_time = 6_400;
        let cases = [
            (
                vec![
                    Ok(WindowFacts::new(pid, true, false)),
                    Ok(WindowFacts::new(pid, true, false)),
                ],
                vec![
                    (1, Err(WindowFailure::AccessDenied)),
                    (2, Err(WindowFailure::AccessDenied)),
                ],
                ProcessOperationError::OperationAccessDenied,
            ),
            (
                vec![
                    Ok(WindowFacts::new(pid, true, false)),
                    Ok(WindowFacts::new(pid, true, false)),
                ],
                vec![
                    (1, Err(WindowFailure::AccessDenied)),
                    (2, Err(WindowFailure::Unavailable)),
                ],
                ProcessOperationError::OperationUnavailable,
            ),
            (
                vec![
                    Ok(WindowFacts::new(pid, true, false)),
                    Ok(WindowFacts::new(pid, true, false)),
                ],
                vec![
                    (1, Err(WindowFailure::Unavailable)),
                    (2, Err(WindowFailure::Unavailable)),
                ],
                ProcessOperationError::OperationUnavailable,
            ),
        ];

        for (facts, posts, expected) in cases {
            let adapter = FakeProcessAdapter::default()
                .with_process(
                    pid,
                    graceful_script(
                        creation_time,
                        vec![
                            Ok(ExitWait::TimedOut),
                            Ok(ExitWait::TimedOut),
                            Ok(ExitWait::TimedOut),
                        ],
                    ),
                )
                .with_window_enumeration(Ok(vec![
                    WindowCandidate::new(1, pid, true, false),
                    WindowCandidate::new(2, pid, true, false),
                ]))
                .with_window_facts(1, vec![facts[0]])
                .with_window_facts(2, vec![facts[1]]);
            let adapter = posts
                .into_iter()
                .fold(adapter, |adapter, (window, outcome)| {
                    adapter.with_post_close_outcomes(window, vec![outcome])
                });
            let clock = FakeMonotonicClock::at(Instant::now());

            assert_eq!(
                terminate(adapter.clone(), pid, creation_time, &clock),
                Err(expected)
            );
            assert_eq!(adapter.terminate_calls(), 0);
        }
    }

    #[test]
    fn partial_post_success_waits_and_later_failures_do_not_override_timeout() {
        let pid = 65;
        let creation_time = 6_500;
        let adapter = FakeProcessAdapter::default()
            .with_process(
                pid,
                graceful_script(
                    creation_time,
                    vec![
                        Ok(ExitWait::TimedOut),
                        Ok(ExitWait::TimedOut),
                        Ok(ExitWait::TimedOut),
                        Ok(ExitWait::TimedOut),
                        Ok(ExitWait::TimedOut),
                        Ok(ExitWait::TimedOut),
                    ],
                ),
            )
            .with_window_enumeration(Ok(vec![
                WindowCandidate::new(1, pid, true, false),
                WindowCandidate::new(2, pid, true, false),
                WindowCandidate::new(3, pid, true, false),
            ]))
            .with_window_facts(1, vec![Ok(WindowFacts::new(pid, true, false))])
            .with_window_facts(2, vec![Ok(WindowFacts::new(pid, true, false))])
            .with_window_facts(3, vec![Err(WindowFailure::Unavailable)])
            .with_post_close_outcomes(1, vec![Ok(())])
            .with_post_close_outcomes(2, vec![Err(WindowFailure::AccessDenied)]);
        let clock = FakeMonotonicClock::at(Instant::now());

        let result = terminate(adapter.clone(), pid, creation_time, &clock);

        assert_eq!(result, Err(ProcessOperationError::GracefulCloseTimedOut));
        assert_eq!(adapter.post_close_calls(), 2);
        assert!(adapter.calls().iter().any(|call| {
            matches!(call, FakeCall::WaitForExit(_, timeout) if *timeout > Duration::ZERO)
        }));
        assert_eq!(adapter.terminate_calls(), 0);
    }

    #[test]
    fn enumeration_consuming_deadline_without_eligible_candidates_is_operation_unavailable() {
        let pid = 72;
        let creation_time = 7_200;
        let base = Instant::now();
        let clock =
            FakeMonotonicClock::with_readings(base, vec![Duration::ZERO, Duration::from_secs(5)]);
        let adapter = FakeProcessAdapter::default()
            .with_process(
                pid,
                graceful_script(creation_time, vec![Ok(ExitWait::TimedOut)]),
            )
            .with_window_enumeration(Ok(vec![WindowCandidate::new(1, pid + 1, true, false)]));

        let result = terminate(adapter.clone(), pid, creation_time, &clock);

        assert_eq!(result, Err(ProcessOperationError::OperationUnavailable));
        assert_eq!(clock.reads(), 2);
        assert_eq!(adapter.post_close_calls(), 0);
        assert_eq!(
            adapter.calls().last(),
            Some(&FakeCall::WaitForExit(pid, Duration::ZERO))
        );
    }

    #[test]
    fn deadline_consumed_before_first_post_returns_operation_unavailable() {
        let pid = 66;
        let creation_time = 6_600;
        let base = Instant::now();
        let clock =
            FakeMonotonicClock::with_readings(base, vec![Duration::ZERO, Duration::from_secs(5)]);
        let adapter = FakeProcessAdapter::default()
            .with_process(
                pid,
                graceful_script(
                    creation_time,
                    vec![Ok(ExitWait::TimedOut), Ok(ExitWait::TimedOut)],
                ),
            )
            .with_window_enumeration(Ok(vec![WindowCandidate::new(1, pid, true, false)]));

        let result = terminate(adapter.clone(), pid, creation_time, &clock);

        assert_eq!(result, Err(ProcessOperationError::OperationUnavailable));
        assert_eq!(clock.reads(), 2);
        assert_eq!(adapter.post_close_calls(), 0);
        assert!(!adapter
            .calls()
            .iter()
            .any(|call| matches!(call, FakeCall::WindowFacts(_))));
        assert_eq!(
            adapter.calls().last(),
            Some(&FakeCall::WaitForExit(pid, Duration::ZERO))
        );
    }

    #[test]
    fn facts_revalidation_crossing_deadline_never_posts_and_is_operation_unavailable() {
        let pid = 73;
        let creation_time = 7_300;
        let base = Instant::now();
        let clock = FakeMonotonicClock::with_readings(
            base,
            vec![Duration::ZERO, Duration::ZERO, Duration::from_secs(5)],
        );
        let adapter = FakeProcessAdapter::default()
            .with_process(
                pid,
                graceful_script(
                    creation_time,
                    vec![
                        Ok(ExitWait::TimedOut),
                        Ok(ExitWait::TimedOut),
                        Ok(ExitWait::TimedOut),
                        Ok(ExitWait::TimedOut),
                    ],
                ),
            )
            .with_window_enumeration(Ok(vec![WindowCandidate::new(1, pid, true, false)]))
            .with_window_facts(1, vec![Ok(WindowFacts::new(pid, true, false))])
            .with_post_close_outcomes(1, vec![Ok(())]);

        let result = terminate(adapter.clone(), pid, creation_time, &clock);

        assert_eq!(result, Err(ProcessOperationError::OperationUnavailable));
        assert_eq!(clock.reads(), 3);
        assert_eq!(adapter.post_close_calls(), 0);
        assert_eq!(
            adapter.calls().last(),
            Some(&FakeCall::WaitForExit(pid, Duration::ZERO))
        );
    }

    #[test]
    fn deadline_consumed_between_posts_stops_posting_and_times_out_after_success() {
        let pid = 67;
        let creation_time = 6_700;
        let base = Instant::now();
        let clock = FakeMonotonicClock::with_readings(
            base,
            vec![
                Duration::ZERO,
                Duration::ZERO,
                Duration::ZERO,
                Duration::from_secs(5),
            ],
        );
        let adapter = FakeProcessAdapter::default()
            .with_process(
                pid,
                graceful_script(
                    creation_time,
                    vec![
                        Ok(ExitWait::TimedOut),
                        Ok(ExitWait::TimedOut),
                        Ok(ExitWait::TimedOut),
                        Ok(ExitWait::TimedOut),
                        Ok(ExitWait::TimedOut),
                    ],
                ),
            )
            .with_window_enumeration(Ok(vec![
                WindowCandidate::new(1, pid, true, false),
                WindowCandidate::new(2, pid, true, false),
            ]))
            .with_window_facts(1, vec![Ok(WindowFacts::new(pid, true, false))])
            .with_post_close_outcomes(1, vec![Ok(())]);

        let result = terminate(adapter.clone(), pid, creation_time, &clock);

        assert_eq!(result, Err(ProcessOperationError::GracefulCloseTimedOut));
        assert_eq!(adapter.post_close_calls(), 1);
        assert!(!adapter
            .calls()
            .iter()
            .any(|call| { matches!(call, FakeCall::WindowFacts(2) | FakeCall::PostClose(2)) }));
        assert!(adapter.calls().iter().any(|call| {
            matches!(call, FakeCall::WaitForExit(_, timeout) if *timeout == Duration::ZERO)
        }));
        assert_eq!(adapter.terminate_calls(), 0);
    }

    #[test]
    fn completion_wait_uses_only_the_shared_deadline_remaining_time() {
        let pid = 71;
        let creation_time = 7_100;
        let base = Instant::now();
        let clock = FakeMonotonicClock::with_readings(
            base,
            vec![
                Duration::ZERO,
                Duration::from_secs(2),
                Duration::from_secs(2),
            ],
        );
        let adapter = FakeProcessAdapter::default()
            .with_process(
                pid,
                graceful_script(
                    creation_time,
                    vec![
                        Ok(ExitWait::TimedOut),
                        Ok(ExitWait::TimedOut),
                        Ok(ExitWait::TimedOut),
                        Ok(ExitWait::TimedOut),
                    ],
                ),
            )
            .with_window_enumeration(Ok(vec![WindowCandidate::new(1, pid, true, false)]))
            .with_window_facts(1, vec![Ok(WindowFacts::new(pid, true, false))])
            .with_post_close_outcomes(1, vec![Ok(())]);

        let result = terminate(adapter.clone(), pid, creation_time, &clock);

        assert_eq!(result, Err(ProcessOperationError::GracefulCloseTimedOut));
        assert!(adapter
            .calls()
            .contains(&FakeCall::WaitForExit(pid, Duration::from_secs(3))));
        assert_eq!(clock.reads(), 4);
        assert_eq!(adapter.terminate_calls(), 0);
    }

    #[test]
    fn completion_requires_observed_exit_and_always_finishes_with_a_zero_poll() {
        let pid = 68;
        let creation_time = 6_800;
        let cases = [
            (
                vec![
                    Ok(ExitWait::TimedOut),
                    Ok(ExitWait::TimedOut),
                    Ok(ExitWait::Exited),
                    Ok(ExitWait::TimedOut),
                ],
                Ok(()),
            ),
            (
                vec![
                    Ok(ExitWait::TimedOut),
                    Ok(ExitWait::TimedOut),
                    Ok(ExitWait::TimedOut),
                    Ok(ExitWait::Exited),
                ],
                Ok(()),
            ),
            (
                vec![
                    Ok(ExitWait::TimedOut),
                    Ok(ExitWait::TimedOut),
                    Ok(ExitWait::TimedOut),
                    Ok(ExitWait::TimedOut),
                ],
                Err(ProcessOperationError::GracefulCloseTimedOut),
            ),
            (
                vec![
                    Ok(ExitWait::TimedOut),
                    Ok(ExitWait::TimedOut),
                    Err(OperationFailure::Unavailable),
                    Err(OperationFailure::Unavailable),
                ],
                Err(ProcessOperationError::CompletionUnknown),
            ),
        ];

        for (waits, expected) in cases {
            let adapter = FakeProcessAdapter::default()
                .with_process(pid, graceful_script(creation_time, waits))
                .with_window_enumeration(Ok(vec![WindowCandidate::new(1, pid, true, false)]))
                .with_window_facts(1, vec![Ok(WindowFacts::new(pid, true, false))])
                .with_post_close_outcomes(1, vec![Ok(())]);
            let clock = FakeMonotonicClock::at(Instant::now());

            assert_eq!(
                terminate(adapter.clone(), pid, creation_time, &clock),
                expected
            );
            assert_eq!(
                adapter.calls().last(),
                Some(&FakeCall::WaitForExit(pid, Duration::ZERO))
            );
            assert_eq!(adapter.post_close_calls(), 1);
            assert_eq!(adapter.terminate_calls(), 0);
        }
    }

    #[test]
    fn enumeration_failures_keep_access_denied_distinct_from_unavailable() {
        for (failure, expected) in [
            (
                OperationFailure::AccessDenied,
                ProcessOperationError::OperationAccessDenied,
            ),
            (
                OperationFailure::ExitedOrChanged,
                ProcessOperationError::OperationUnavailable,
            ),
            (
                OperationFailure::Unavailable,
                ProcessOperationError::OperationUnavailable,
            ),
        ] {
            let pid = 69;
            let creation_time = 6_900;
            let adapter = FakeProcessAdapter::default()
                .with_process(pid, graceful_script(creation_time, vec![]))
                .with_window_enumeration(Err(failure));
            let clock = FakeMonotonicClock::at(Instant::now());

            assert_eq!(
                terminate(adapter.clone(), pid, creation_time, &clock),
                Err(expected)
            );
            assert_eq!(adapter.post_close_calls(), 0);
            assert_eq!(adapter.terminate_calls(), 0);
        }
    }

    #[test]
    fn graceful_close_event_order_is_exact_and_retains_only_the_operation_handle() {
        let pid = 70;
        let creation_time = 7_000;
        let adapter = FakeProcessAdapter::default()
            .with_process(
                pid,
                graceful_script(
                    creation_time,
                    vec![
                        Ok(ExitWait::TimedOut),
                        Ok(ExitWait::TimedOut),
                        Ok(ExitWait::TimedOut),
                        Ok(ExitWait::Exited),
                    ],
                ),
            )
            .with_window_enumeration(Ok(vec![WindowCandidate::new(7, pid, true, false)]))
            .with_window_facts(7, vec![Ok(WindowFacts::new(pid, true, false))])
            .with_post_close_outcomes(7, vec![Ok(())]);
        let clock = FakeMonotonicClock::at(Instant::now());

        assert_eq!(
            terminate(adapter.clone(), pid, creation_time, &clock),
            Ok(())
        );
        assert_eq!(
            adapter.calls(),
            vec![
                FakeCall::OpenObservation(pid),
                FakeCall::CreationTime(pid),
                FakeCall::Liveness(pid),
                FakeCall::Protection(pid),
                FakeCall::OpenOperation(pid),
                FakeCall::CreationTime(pid),
                FakeCall::Liveness(pid),
                FakeCall::Protection(pid),
                FakeCall::EnumerateTopLevelWindows,
                FakeCall::WaitForExit(pid, Duration::ZERO),
                FakeCall::WindowFacts(7),
                FakeCall::PostClose(7),
                FakeCall::WaitForExit(pid, Duration::ZERO),
                FakeCall::WaitForExit(pid, Duration::from_secs(5)),
                FakeCall::WaitForExit(pid, Duration::ZERO),
            ]
        );
        for event in adapter.events() {
            if let FakeEvent::Call {
                call:
                    FakeCall::EnumerateTopLevelWindows
                    | FakeCall::WindowFacts(_)
                    | FakeCall::PostClose(_)
                    | FakeCall::WaitForExit(_, _),
                active_handles,
            } = event
            {
                assert_eq!(active_handles, vec![2]);
            }
        }
        assert_eq!(
            adapter.requested_operation_access(),
            vec![OperationAccess::GRACEFUL]
        );
        assert_eq!(adapter.terminate_calls(), 0);
        assert_eq!(adapter.opened(), 2);
        assert_eq!(adapter.dropped(), 2);
    }
}

mod operation_force_terminate {
    use super::*;

    fn force_script(
        creation_time: u64,
        terminations: Vec<Result<(), OperationFailure>>,
        waits: Vec<Result<ExitWait, OperationFailure>>,
    ) -> FakeProcessScript {
        FakeProcessScript {
            open: vec![Ok(())].into(),
            operation_open: vec![Ok(())].into(),
            creation_times: vec![Ok(creation_time), Ok(creation_time)].into(),
            liveness: vec![Ok(Liveness::Alive), Ok(Liveness::Alive)].into(),
            protections: vec![
                Ok(ProcessProtection::new(false, false)),
                Ok(ProcessProtection::new(false, false)),
            ]
            .into(),
            terminations: terminations.into(),
            waits: waits.into(),
            ..FakeProcessScript::default()
        }
    }

    fn force_terminate(
        adapter: FakeProcessAdapter,
        pid: u32,
        creation_time: u64,
        clock: &FakeMonotonicClock,
    ) -> Result<(), ProcessOperationError> {
        let authority = authority(adapter);
        let admission = authority
            .admit(&target(pid, creation_time), ProcessIntent::ForceTerminate)
            .unwrap();
        authority.force_terminate_with_clock(admission, clock)
    }

    #[test]
    fn pre_call_concurrent_exit_succeeds_without_terminating() {
        let pid = 74;
        let creation_time = 7_400;
        let adapter = FakeProcessAdapter::default().with_process(
            pid,
            force_script(creation_time, vec![], vec![Ok(ExitWait::Exited)]),
        );
        let clock = FakeMonotonicClock::at(Instant::now());

        let result = force_terminate(adapter.clone(), pid, creation_time, &clock);

        assert_eq!(result, Ok(()));
        assert_eq!(adapter.terminate_calls(), 0);
        assert_eq!(
            adapter.requested_operation_access(),
            vec![OperationAccess::FORCE]
        );
        assert_eq!(
            adapter.calls().last(),
            Some(&FakeCall::WaitForExit(pid, Duration::ZERO))
        );
        assert_eq!(adapter.opened(), 2);
        assert_eq!(adapter.dropped(), 2);
    }

    #[test]
    fn terminate_is_called_once_with_exit_code_one_and_immediate_exit_succeeds() {
        let pid = 75;
        let creation_time = 7_500;
        let adapter = FakeProcessAdapter::default().with_process(
            pid,
            force_script(
                creation_time,
                vec![Ok(())],
                vec![
                    Ok(ExitWait::TimedOut),
                    Ok(ExitWait::Exited),
                    Ok(ExitWait::TimedOut),
                ],
            ),
        );
        let clock = FakeMonotonicClock::at(Instant::now());

        let result = force_terminate(adapter.clone(), pid, creation_time, &clock);

        assert_eq!(result, Ok(()));
        assert_eq!(adapter.terminate_calls(), 1);
        assert_eq!(adapter.terminate_exit_codes(), vec![1]);
        assert_eq!(
            &adapter.calls()[8..],
            [
                FakeCall::WaitForExit(pid, Duration::ZERO),
                FakeCall::TerminateProcess(pid),
                FakeCall::WaitForExit(pid, Duration::from_secs(5)),
                FakeCall::WaitForExit(pid, Duration::ZERO),
            ]
        );
        assert_eq!(adapter.opened(), 2);
        assert_eq!(adapter.dropped(), 2);
    }

    #[test]
    fn delayed_exit_observed_by_the_final_zero_poll_succeeds() {
        let pid = 76;
        let creation_time = 7_600;
        let adapter = FakeProcessAdapter::default().with_process(
            pid,
            force_script(
                creation_time,
                vec![Ok(())],
                vec![
                    Ok(ExitWait::TimedOut),
                    Ok(ExitWait::TimedOut),
                    Ok(ExitWait::Exited),
                ],
            ),
        );
        let clock = FakeMonotonicClock::at(Instant::now());

        let result = force_terminate(adapter.clone(), pid, creation_time, &clock);

        assert_eq!(result, Ok(()));
        assert_eq!(adapter.terminate_calls(), 1);
        assert_eq!(
            adapter.calls().last(),
            Some(&FakeCall::WaitForExit(pid, Duration::ZERO))
        );
        assert_eq!(adapter.opened(), 2);
        assert_eq!(adapter.dropped(), 2);
    }

    #[test]
    fn successful_terminate_deadline_exhaustion_is_termination_pending() {
        let pid = 77;
        let creation_time = 7_700;
        let base = Instant::now();
        let clock =
            FakeMonotonicClock::with_readings(base, vec![Duration::ZERO, Duration::from_secs(5)]);
        let adapter = FakeProcessAdapter::default().with_process(
            pid,
            force_script(
                creation_time,
                vec![Ok(())],
                vec![
                    Ok(ExitWait::TimedOut),
                    Ok(ExitWait::TimedOut),
                    Ok(ExitWait::TimedOut),
                ],
            ),
        );

        let result = force_terminate(adapter.clone(), pid, creation_time, &clock);

        assert_eq!(result, Err(ProcessOperationError::TerminationPending));
        assert_eq!(adapter.terminate_calls(), 1);
        assert_eq!(
            &adapter.calls()[10..],
            [
                FakeCall::WaitForExit(pid, Duration::ZERO),
                FakeCall::WaitForExit(pid, Duration::ZERO),
            ]
        );
        assert_eq!(adapter.opened(), 2);
        assert_eq!(adapter.dropped(), 2);
    }

    #[test]
    fn successful_terminate_wait_inspection_failure_is_completion_unknown() {
        let pid = 78;
        let creation_time = 7_800;
        let adapter = FakeProcessAdapter::default().with_process(
            pid,
            force_script(
                creation_time,
                vec![Ok(())],
                vec![
                    Ok(ExitWait::TimedOut),
                    Err(OperationFailure::Unavailable),
                    Ok(ExitWait::TimedOut),
                ],
            ),
        );
        let clock = FakeMonotonicClock::at(Instant::now());

        let result = force_terminate(adapter.clone(), pid, creation_time, &clock);

        assert_eq!(result, Err(ProcessOperationError::CompletionUnknown));
        assert_eq!(adapter.terminate_calls(), 1);
        assert_eq!(
            adapter.calls().last(),
            Some(&FakeCall::WaitForExit(pid, Duration::ZERO))
        );
        assert_eq!(adapter.opened(), 2);
        assert_eq!(adapter.dropped(), 2);
    }

    #[test]
    fn completion_wait_uses_only_the_deadline_remaining_duration() {
        let pid = 79;
        let creation_time = 7_900;
        let base = Instant::now();
        let clock =
            FakeMonotonicClock::with_readings(base, vec![Duration::ZERO, Duration::from_secs(2)]);
        let adapter = FakeProcessAdapter::default().with_process(
            pid,
            force_script(
                creation_time,
                vec![Ok(())],
                vec![
                    Ok(ExitWait::TimedOut),
                    Ok(ExitWait::TimedOut),
                    Ok(ExitWait::TimedOut),
                ],
            ),
        );

        let result = force_terminate(adapter.clone(), pid, creation_time, &clock);

        assert_eq!(result, Err(ProcessOperationError::TerminationPending));
        assert_eq!(clock.reads(), 2);
        assert_eq!(
            &adapter.calls()[8..],
            [
                FakeCall::WaitForExit(pid, Duration::ZERO),
                FakeCall::TerminateProcess(pid),
                FakeCall::WaitForExit(pid, Duration::from_secs(3)),
                FakeCall::WaitForExit(pid, Duration::ZERO),
            ]
        );
        assert_eq!(adapter.opened(), 2);
        assert_eq!(adapter.dropped(), 2);
    }

    #[test]
    fn failed_terminate_with_signaled_repoll_succeeds() {
        let pid = 80;
        let creation_time = 8_000;
        let adapter = FakeProcessAdapter::default().with_process(
            pid,
            force_script(
                creation_time,
                vec![Err(OperationFailure::AccessDenied)],
                vec![Ok(ExitWait::TimedOut), Ok(ExitWait::Exited)],
            ),
        );
        let clock = FakeMonotonicClock::at(Instant::now());

        let result = force_terminate(adapter.clone(), pid, creation_time, &clock);

        assert_eq!(result, Ok(()));
        assert_eq!(adapter.terminate_calls(), 1);
        assert_eq!(
            &adapter.calls()[8..],
            [
                FakeCall::WaitForExit(pid, Duration::ZERO),
                FakeCall::TerminateProcess(pid),
                FakeCall::WaitForExit(pid, Duration::ZERO),
            ]
        );
        assert_eq!(adapter.opened(), 2);
        assert_eq!(adapter.dropped(), 2);
    }

    #[test]
    fn failed_terminate_with_alive_repoll_maps_access_and_other_failures() {
        for (failure, expected) in [
            (
                OperationFailure::AccessDenied,
                ProcessOperationError::OperationAccessDenied,
            ),
            (
                OperationFailure::ExitedOrChanged,
                ProcessOperationError::OperationUnavailable,
            ),
            (
                OperationFailure::Unavailable,
                ProcessOperationError::OperationUnavailable,
            ),
        ] {
            let pid = 81;
            let creation_time = 8_100;
            let adapter = FakeProcessAdapter::default().with_process(
                pid,
                force_script(
                    creation_time,
                    vec![Err(failure), Ok(())],
                    vec![Ok(ExitWait::TimedOut), Ok(ExitWait::TimedOut)],
                ),
            );
            let clock = FakeMonotonicClock::at(Instant::now());

            let result = force_terminate(adapter.clone(), pid, creation_time, &clock);

            assert_eq!(result, Err(expected));
            assert_eq!(adapter.terminate_calls(), 1);
            assert_eq!(adapter.terminate_exit_codes(), vec![1]);
            assert_eq!(adapter.opened(), 2);
            assert_eq!(adapter.dropped(), 2);
        }
    }

    #[test]
    fn failed_terminate_with_failed_repoll_is_completion_unknown() {
        let pid = 82;
        let creation_time = 8_200;
        let adapter = FakeProcessAdapter::default().with_process(
            pid,
            force_script(
                creation_time,
                vec![Err(OperationFailure::AccessDenied)],
                vec![Ok(ExitWait::TimedOut), Err(OperationFailure::Unavailable)],
            ),
        );
        let clock = FakeMonotonicClock::at(Instant::now());

        let result = force_terminate(adapter.clone(), pid, creation_time, &clock);

        assert_eq!(result, Err(ProcessOperationError::CompletionUnknown));
        assert_eq!(adapter.terminate_calls(), 1);
        assert_eq!(adapter.opened(), 2);
        assert_eq!(adapter.dropped(), 2);
    }

    #[test]
    fn initial_poll_failure_prevents_terminate_and_maps_availability() {
        for (failure, expected) in [
            (
                OperationFailure::AccessDenied,
                ProcessOperationError::OperationAccessDenied,
            ),
            (
                OperationFailure::ExitedOrChanged,
                ProcessOperationError::OperationUnavailable,
            ),
            (
                OperationFailure::Unavailable,
                ProcessOperationError::OperationUnavailable,
            ),
        ] {
            let pid = 83;
            let creation_time = 8_300;
            let adapter = FakeProcessAdapter::default().with_process(
                pid,
                force_script(creation_time, vec![Ok(())], vec![Err(failure)]),
            );
            let clock = FakeMonotonicClock::at(Instant::now());

            let result = force_terminate(adapter.clone(), pid, creation_time, &clock);

            assert_eq!(result, Err(expected));
            assert_eq!(adapter.terminate_calls(), 0);
            assert_eq!(adapter.opened(), 2);
            assert_eq!(adapter.dropped(), 2);
        }
    }

    #[test]
    fn terminate_failure_is_not_retried_or_elevated() {
        let pid = 84;
        let creation_time = 8_400;
        let adapter = FakeProcessAdapter::default().with_process(
            pid,
            force_script(
                creation_time,
                vec![Err(OperationFailure::AccessDenied), Ok(())],
                vec![Ok(ExitWait::TimedOut), Ok(ExitWait::TimedOut)],
            ),
        );
        let clock = FakeMonotonicClock::at(Instant::now());

        let result = force_terminate(adapter.clone(), pid, creation_time, &clock);

        assert_eq!(result, Err(ProcessOperationError::OperationAccessDenied));
        assert_eq!(adapter.terminate_calls(), 1);
        let source = include_str!("operation.rs");
        let force_source = source
            .split("fn force_terminate_with_clock")
            .nth(1)
            .unwrap()
            .split("fn with_final_operation_admission")
            .next()
            .unwrap();
        assert!(!force_source.contains("elevat"));
        assert_eq!(adapter.opened(), 2);
        assert_eq!(adapter.dropped(), 2);
    }

    #[test]
    fn independent_lease_after_exit_fails_during_final_admission_with_exact_drops() {
        let pid = 85;
        let creation_time = 8_500;
        let safe = Ok(ProcessProtection::new(false, false));
        let adapter = FakeProcessAdapter::default().with_process(
            pid,
            FakeProcessScript {
                open: vec![Ok(()), Ok(())].into(),
                operation_open: vec![Ok(()), Ok(())].into(),
                creation_times: vec![Ok(creation_time); 4].into(),
                liveness: vec![
                    Ok(Liveness::Alive),
                    Ok(Liveness::Alive),
                    Ok(Liveness::Alive),
                    Ok(Liveness::Exited),
                ]
                .into(),
                protections: vec![safe, safe, safe].into(),
                terminations: vec![Ok(())].into(),
                waits: vec![
                    Ok(ExitWait::TimedOut),
                    Ok(ExitWait::Exited),
                    Ok(ExitWait::Exited),
                ]
                .into(),
                ..FakeProcessScript::default()
            },
        );
        let authority = authority(adapter.clone());
        let first = authority
            .admit(&target(pid, creation_time), ProcessIntent::ForceTerminate)
            .unwrap();
        let second = authority
            .admit(&target(pid, creation_time), ProcessIntent::ForceTerminate)
            .unwrap();
        let clock = FakeMonotonicClock::at(Instant::now());

        let first_result = authority.force_terminate_with_clock(first, &clock);
        let second_result = authority.force_terminate_with_clock(second, &clock);

        assert_eq!(first_result, Ok(()));
        assert_eq!(second_result, Err(ProcessOperationError::TargetExited));
        assert_eq!(adapter.terminate_calls(), 1);
        assert_eq!(
            adapter.requested_operation_access(),
            vec![OperationAccess::FORCE, OperationAccess::FORCE]
        );
        assert_eq!(adapter.opened(), 4);
        assert_eq!(adapter.dropped(), 4);
        assert_eq!(
            adapter.handle_events(),
            vec![
                FakeHandleEvent::Opened {
                    handle_id: 1,
                    object_id: 1,
                    pid,
                    kind: FakeHandleKind::Observation,
                },
                FakeHandleEvent::Opened {
                    handle_id: 2,
                    object_id: 1,
                    pid,
                    kind: FakeHandleKind::Observation,
                },
                FakeHandleEvent::Opened {
                    handle_id: 3,
                    object_id: 1,
                    pid,
                    kind: FakeHandleKind::Operation,
                },
                FakeHandleEvent::Dropped {
                    handle_id: 1,
                    object_id: 1,
                    pid,
                    kind: FakeHandleKind::Observation,
                },
                FakeHandleEvent::Dropped {
                    handle_id: 3,
                    object_id: 1,
                    pid,
                    kind: FakeHandleKind::Operation,
                },
                FakeHandleEvent::Opened {
                    handle_id: 4,
                    object_id: 1,
                    pid,
                    kind: FakeHandleKind::Operation,
                },
                FakeHandleEvent::Dropped {
                    handle_id: 4,
                    object_id: 1,
                    pid,
                    kind: FakeHandleKind::Operation,
                },
                FakeHandleEvent::Dropped {
                    handle_id: 2,
                    object_id: 1,
                    pid,
                    kind: FakeHandleKind::Observation,
                },
            ]
        );
    }

    struct NestedForceClock<'a> {
        now: Instant,
        authority: &'a CoreAuthority<FakeProcessAdapter>,
        admission: RefCell<Option<CoreAdmission<FakeHandle>>>,
        result: RefCell<Option<Result<(), ProcessOperationError>>>,
    }

    impl MonotonicClock for NestedForceClock<'_> {
        fn now(&self) -> Instant {
            if let Some(admission) = self.admission.borrow_mut().take() {
                let nested_clock = FakeMonotonicClock::at(self.now);
                let result = self
                    .authority
                    .force_terminate_with_clock(admission, &nested_clock);
                self.result.replace(Some(result));
            }
            self.now
        }
    }

    #[test]
    fn independent_post_admission_lease_observes_exit_without_second_terminate() {
        let pid = 86;
        let creation_time = 8_600;
        let safe = Ok(ProcessProtection::new(false, false));
        let adapter = FakeProcessAdapter::default().with_process(
            pid,
            FakeProcessScript {
                open: vec![Ok(()), Ok(())].into(),
                operation_open: vec![Ok(()), Ok(())].into(),
                creation_times: vec![Ok(creation_time); 4].into(),
                liveness: vec![Ok(Liveness::Alive); 4].into(),
                protections: vec![safe; 4].into(),
                terminations: vec![Ok(())].into(),
                waits: vec![
                    Ok(ExitWait::TimedOut),
                    Ok(ExitWait::Exited),
                    Ok(ExitWait::Exited),
                    Ok(ExitWait::Exited),
                ]
                .into(),
                ..FakeProcessScript::default()
            },
        );
        let authority = authority(adapter.clone());
        let outer = authority
            .admit(&target(pid, creation_time), ProcessIntent::ForceTerminate)
            .unwrap();
        let nested = authority
            .admit(&target(pid, creation_time), ProcessIntent::ForceTerminate)
            .unwrap();
        let clock = NestedForceClock {
            now: Instant::now(),
            authority: &authority,
            admission: RefCell::new(Some(nested)),
            result: RefCell::new(None),
        };

        let outer_result = authority.force_terminate_with_clock(outer, &clock);

        assert_eq!(outer_result, Ok(()));
        assert_eq!(*clock.result.borrow(), Some(Ok(())));
        assert_eq!(adapter.terminate_calls(), 1);
        assert_eq!(
            adapter.requested_operation_access(),
            vec![OperationAccess::FORCE, OperationAccess::FORCE]
        );
        assert_eq!(adapter.opened(), 4);
        assert_eq!(adapter.dropped(), 4);
        assert_eq!(
            adapter.handle_events(),
            vec![
                FakeHandleEvent::Opened {
                    handle_id: 1,
                    object_id: 1,
                    pid,
                    kind: FakeHandleKind::Observation,
                },
                FakeHandleEvent::Opened {
                    handle_id: 2,
                    object_id: 1,
                    pid,
                    kind: FakeHandleKind::Observation,
                },
                FakeHandleEvent::Opened {
                    handle_id: 3,
                    object_id: 1,
                    pid,
                    kind: FakeHandleKind::Operation,
                },
                FakeHandleEvent::Dropped {
                    handle_id: 1,
                    object_id: 1,
                    pid,
                    kind: FakeHandleKind::Observation,
                },
                FakeHandleEvent::Opened {
                    handle_id: 4,
                    object_id: 1,
                    pid,
                    kind: FakeHandleKind::Operation,
                },
                FakeHandleEvent::Dropped {
                    handle_id: 2,
                    object_id: 1,
                    pid,
                    kind: FakeHandleKind::Observation,
                },
                FakeHandleEvent::Dropped {
                    handle_id: 4,
                    object_id: 1,
                    pid,
                    kind: FakeHandleKind::Operation,
                },
                FakeHandleEvent::Dropped {
                    handle_id: 3,
                    object_id: 1,
                    pid,
                    kind: FakeHandleKind::Operation,
                },
            ]
        );
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
        sync::atomic::{AtomicBool, Ordering},
        thread,
        time::{Duration, Instant},
    };

    use windows::{
        core::{w, Error as WindowsError, HRESULT, PCWSTR},
        Win32::{
            Foundation::{
                ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, ERROR_INVALID_WINDOW_HANDLE,
                ERROR_NOT_FOUND, FILETIME, HANDLE, HWND, LPARAM, LRESULT, WAIT_FAILED,
                WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM,
            },
            System::Threading::{
                GetCurrentProcessId, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
                PROCESS_TERMINATE,
            },
            UI::WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
                PostQuitMessage, RegisterClassW, ShowWindow, TranslateMessage, CW_USEDEFAULT, MSG,
                SW_SHOWNOACTIVATE, WM_CLOSE, WM_DESTROY, WNDCLASSW, WS_EX_NOACTIVATE,
                WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW,
            },
        },
    };

    use super::*;
    use crate::process_control::windows::{
        classify_observation_error, classify_operation_error, classify_path_query_error,
        classify_window_error, collect_candidates_with, collect_window_candidates_with,
        filetime_to_u64, find_reenumerated_name, finite_timeout_millis,
        generate_host_instance_with, liveness_from_wait_result, open_observation_with,
        open_operation_with, wait_result_with, window_facts_with, EnumerationStep, OwnedHandle,
        WindowsProcessAdapter, WindowsWindowToken,
    };
    use crate::process_control::{
        ProcessAuthority, ProcessAuthorityInitError, ProcessError, ProcessIntent,
        ProcessOperationError,
    };

    const FIXTURE_TEST: &str = "process_control::tests::windows_tests::windows_fixture_child";
    const FIXTURE_MARKER: &str = "UIPILOT_PROCESS_CONTROL_TEST_CHILD";
    const FIXTURE_MODE: &str = "UIPILOT_PROCESS_CONTROL_TEST_MODE";
    const FIXTURE_WINDOW_CLASS: PCWSTR = w!("UiPilotProcessControlTestWindow");
    static FIXTURE_IGNORES_CLOSE: AtomicBool = AtomicBool::new(false);

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
    fn windows_operation_open_uses_exact_access_and_never_inherits() {
        let cases = [
            (
                OperationAccess::GRACEFUL,
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            ),
            (
                OperationAccess::FORCE,
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | PROCESS_TERMINATE,
            ),
        ];

        for (profile, expected_access) in cases {
            let mut captured = None;
            let result = open_operation_with(profile, 7_003, |access, inherit, pid| {
                captured = Some((access, inherit, pid));
                Err::<(), _>(OperationFailure::AccessDenied)
            });

            assert_eq!(result, Err(OperationFailure::AccessDenied));
            assert_eq!(captured, Some((expected_access, false, 7_003)));
        }
    }

    #[test]
    fn windows_operation_window_wrapper_collection_and_fact_refresh_are_exact() {
        let first = WindowsWindowToken::from_raw(HWND(11usize as _));
        let gone = WindowsWindowToken::from_raw(HWND(12usize as _));
        let refreshed = window_facts_with(first, |window| {
            assert_eq!(window.raw(), HWND(11usize as _));
            Ok((7_004, true, false))
        })
        .unwrap();

        assert_eq!(refreshed, WindowFacts::new(7_004, true, false));
        assert_eq!(first.raw(), HWND(11usize as _));
        assert_eq!(
            collect_window_candidates_with(vec![first, gone], |window| {
                if window == first {
                    Ok(WindowFacts::new(7_004, true, false))
                } else {
                    Err(WindowFailure::Gone)
                }
            }),
            Ok(vec![WindowCandidate::new(first, 7_004, true, false)])
        );

        for (failure, expected) in [
            (WindowFailure::AccessDenied, OperationFailure::AccessDenied),
            (WindowFailure::Unavailable, OperationFailure::Unavailable),
        ] {
            assert_eq!(
                collect_window_candidates_with(vec![first], |_| Err(failure)),
                Err(expected)
            );
        }
    }

    #[test]
    fn windows_operation_error_classification_is_context_specific_and_redacted() {
        let error = |code| WindowsError::from_hresult(HRESULT::from_win32(code));

        assert_eq!(
            classify_operation_error(error(ERROR_ACCESS_DENIED.0)),
            OperationFailure::AccessDenied
        );
        assert_eq!(
            classify_operation_error(error(ERROR_INVALID_PARAMETER.0)),
            OperationFailure::ExitedOrChanged
        );
        assert_eq!(
            classify_window_error(error(ERROR_INVALID_WINDOW_HANDLE.0)),
            WindowFailure::Gone
        );
        assert_eq!(
            classify_window_error(error(ERROR_ACCESS_DENIED.0)),
            WindowFailure::AccessDenied
        );
        assert_eq!(
            classify_window_error(error(1234)),
            WindowFailure::Unavailable
        );
    }

    #[test]
    fn windows_operation_waits_use_zero_or_rounded_finite_milliseconds() {
        assert_eq!(finite_timeout_millis(Duration::ZERO), 0);
        assert_eq!(finite_timeout_millis(Duration::from_nanos(1)), 1);
        assert_eq!(finite_timeout_millis(Duration::from_millis(1250)), 1250);
        assert_eq!(
            finite_timeout_millis(Duration::from_millis(u64::from(u32::MAX))),
            u32::MAX - 1
        );

        let mut captured = Vec::new();
        assert_eq!(
            wait_result_with(
                Duration::ZERO,
                |timeout| {
                    captured.push(timeout);
                    WAIT_TIMEOUT
                },
                || panic!("last error is not read for WAIT_TIMEOUT"),
            ),
            Ok(ExitWait::TimedOut)
        );
        assert_eq!(
            wait_result_with(
                Duration::from_micros(1_001),
                |timeout| {
                    captured.push(timeout);
                    WAIT_OBJECT_0
                },
                || panic!("last error is not read for WAIT_OBJECT_0"),
            ),
            Ok(ExitWait::Exited)
        );
        assert_eq!(captured, vec![0, 2]);
        assert_eq!(
            wait_result_with(
                Duration::from_secs(1),
                |_| WAIT_FAILED,
                || WindowsError::from_hresult(HRESULT::from_win32(ERROR_ACCESS_DENIED.0)),
            ),
            Err(OperationFailure::AccessDenied)
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
                FakeCall::OpenObservation(pid),
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
    fn windows_operation_current_process_path_reconciles_and_self_is_protected() {
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
    fn windows_operation_current_process_changed_creation_time_is_target_changed() {
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
    fn windows_operation_graceful_helper_exits_through_wm_close() {
        let mut child = FixtureChild::spawn_mode("graceful-window");
        let authority = ProcessAuthority::new().unwrap();
        let record = snapshot_record_for_pid(&authority, child.id(), Duration::from_secs(5));
        wait_for_eligible_window(child.id(), Duration::from_secs(5));
        let admission = authority
            .admit(record.target(), ProcessIntent::Terminate)
            .unwrap();

        assert_eq!(authority.terminate(admission), Ok(()));
        assert!(child.wait_for_exit(Duration::from_secs(2)).is_some());
        child.cleanup();
    }

    #[test]
    fn windows_operation_windowless_helper_is_unavailable_without_side_effect() {
        let mut child = FixtureChild::spawn(Duration::from_secs(30));
        let authority = ProcessAuthority::new().unwrap();
        let record = snapshot_record_for_pid(&authority, child.id(), Duration::from_secs(5));
        wait_for_no_eligible_window(child.id(), Duration::from_millis(200));
        let admission = authority
            .admit(record.target(), ProcessIntent::Terminate)
            .unwrap();

        assert_eq!(
            authority.terminate(admission),
            Err(ProcessOperationError::GracefulCloseUnavailable)
        );
        assert!(matches!(child.child.try_wait(), Ok(None)));
        child.cleanup();
    }

    #[test]
    fn windows_operation_ignored_close_times_out_then_owned_child_is_cleaned() {
        let mut child = FixtureChild::spawn_mode("ignore-close-window");
        let authority = ProcessAuthority::new().unwrap();
        let record = snapshot_record_for_pid(&authority, child.id(), Duration::from_secs(5));
        wait_for_eligible_window(child.id(), Duration::from_secs(5));
        let admission = authority
            .admit(record.target(), ProcessIntent::Terminate)
            .unwrap();
        let started = Instant::now();

        assert_eq!(
            authority.terminate(admission),
            Err(ProcessOperationError::GracefulCloseTimedOut)
        );
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_millis(4_500),
            "elapsed {elapsed:?}"
        );
        assert!(elapsed <= Duration::from_secs(8), "elapsed {elapsed:?}");
        assert!(matches!(child.child.try_wait(), Ok(None)));
        child.cleanup();
        assert!(child.wait_for_exit(Duration::ZERO).is_some());
    }

    #[test]
    fn windows_operation_force_terminates_and_exact_child_signals() {
        let mut child = FixtureChild::spawn(Duration::from_secs(30));
        let authority = ProcessAuthority::new().unwrap();
        let record = snapshot_record_for_pid(&authority, child.id(), Duration::from_secs(5));
        wait_for_no_eligible_window(child.id(), Duration::from_millis(200));
        let admission = authority
            .admit(record.target(), ProcessIntent::ForceTerminate)
            .unwrap();

        assert_eq!(authority.force_terminate(admission), Ok(()));
        assert!(child.wait_for_exit(Duration::from_secs(2)).is_some());
        child.cleanup();
    }

    #[test]
    fn windows_operation_source_requires_scoped_primitives_and_forbids_fallbacks() {
        let source = include_str!("windows.rs");
        let module = include_str!("mod.rs");

        for required in [
            "OpenProcess(access, false, pid)",
            "EnumWindows(",
            "GetWindowThreadProcessId(window.raw(), Some(&mut pid))",
            "IsWindowVisible(window.raw())",
            "GetWindow(window.raw(), GW_OWNER)",
            "PostMessageW(",
            "WM_CLOSE,",
            "TerminateProcess(handle.0.get(), exit_code)",
            "WaitForSingleObject(handle.0.get(), timeout)",
            "PROCESS_QUERY_LIMITED_INFORMATION",
            "PROCESS_SYNCHRONIZE",
            "PROCESS_TERMINATE",
        ] {
            assert!(source.contains(required), "missing required {required}");
        }
        for forbidden in [
            "PowerShell",
            "powershell",
            "WMI",
            "wmic",
            "taskkill",
            "ShellExecute",
            "std::process::Command",
            "NtQuery",
            "NtTerminate",
            "ZwTerminate",
            "EndTask",
            "endtask",
            "SeDebugPrivilege",
            "GetExitCodeProcess",
            "PROCESS_ALL_ACCESS",
            "PROCESS_CREATE_THREAD",
        ] {
            assert!(!source.contains(forbidden), "found forbidden {forbidden}");
        }
        assert!(source.contains("BCryptGenRandom(None"));
        assert!(source.contains("BCRYPT_USE_SYSTEM_PREFERRED_RNG"));
        assert!(module.contains("cfg(not(windows))"));
        assert!(module.contains("SystemMonotonicClock"));
        assert!(module.contains("terminate_with_clock"));
        assert!(module.contains("force_terminate_with_clock"));
        assert!(!module.contains("raw_handle"));
    }

    #[test]
    #[ignore = "spawned explicitly by Windows process-control tests"]
    fn windows_fixture_child() {
        assert_eq!(std::env::var(FIXTURE_MARKER).as_deref(), Ok("1"));
        match std::env::var(FIXTURE_MODE).as_deref() {
            Ok("sleep") => {
                let delay = std::env::var("UIPILOT_PROCESS_CONTROL_TEST_DELAY_MS")
                    .unwrap()
                    .parse::<u64>()
                    .unwrap();
                thread::sleep(Duration::from_millis(delay));
                std::process::exit(259);
            }
            Ok("graceful-window") => run_fixture_window(false),
            Ok("ignore-close-window") => run_fixture_window(true),
            mode => panic!("unsupported fixture mode {mode:?}"),
        }
    }

    fn run_fixture_window(ignore_close: bool) {
        FIXTURE_IGNORES_CLOSE.store(ignore_close, Ordering::Relaxed);
        let class = WNDCLASSW {
            lpfnWndProc: Some(fixture_window_proc),
            lpszClassName: FIXTURE_WINDOW_CLASS,
            ..Default::default()
        };
        assert_ne!(unsafe { RegisterClassW(&class) }, 0);
        let window = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                FIXTURE_WINDOW_CLASS,
                w!("UiPilot process control fixture"),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                320,
                200,
                None,
                None,
                None,
                None,
            )
        }
        .unwrap();
        unsafe {
            let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
        }

        let mut message = MSG::default();
        loop {
            let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
            assert_ne!(result, -1, "fixture message loop failed");
            if result == 0 {
                break;
            }
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    unsafe extern "system" fn fixture_window_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_CLOSE if FIXTURE_IGNORES_CLOSE.load(Ordering::Relaxed) => LRESULT(0),
            WM_CLOSE => {
                let _ = unsafe { DestroyWindow(window) };
                LRESULT(0)
            }
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
        }
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

    fn wait_for_eligible_window(pid: u32, timeout: Duration) {
        let adapter = WindowsProcessAdapter;
        let deadline = Instant::now() + timeout;
        loop {
            let eligible = adapter
                .enumerate_top_level_windows()
                .unwrap()
                .into_iter()
                .filter(|candidate| candidate.pid() == pid)
                .filter_map(|candidate| adapter.window_facts(candidate.token()).ok())
                .filter(|facts| facts.pid() == pid && facts.visible() && !facts.has_owner())
                .count();
            if eligible == 1 {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "fixture did not expose exactly one eligible window; found {eligible}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for_no_eligible_window(pid: u32, settle: Duration) {
        let adapter = WindowsProcessAdapter;
        let deadline = Instant::now() + settle;
        loop {
            let eligible = adapter
                .enumerate_top_level_windows()
                .unwrap()
                .into_iter()
                .filter(|candidate| candidate.pid() == pid)
                .filter_map(|candidate| adapter.window_facts(candidate.token()).ok())
                .any(|facts| facts.pid() == pid && facts.visible() && !facts.has_owner());
            assert!(!eligible, "windowless fixture exposed an eligible window");
            if Instant::now() >= deadline {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    struct FixtureChild {
        child: Child,
    }

    impl FixtureChild {
        fn spawn(delay: Duration) -> Self {
            Self::spawn_with("sleep", Some(delay))
        }

        fn spawn_mode(mode: &str) -> Self {
            Self::spawn_with(mode, None)
        }

        fn spawn_with(mode: &str, delay: Option<Duration>) -> Self {
            let child = Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", FIXTURE_TEST, "--test-threads=1"])
                .env(FIXTURE_MARKER, "1")
                .env(FIXTURE_MODE, mode)
                .env(
                    "UIPILOT_PROCESS_CONTROL_TEST_DELAY_MS",
                    delay.unwrap_or(Duration::ZERO).as_millis().to_string(),
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
