//! Transport-neutral request authority. Permits cover actual backend/reply/host
//! submission work; drain never cancels an OS operation or proves a sandbox.
use super::data_call_gate::{PluginDataCallGate, PluginDataCallIdentity, PluginDataCallLease};
use super::request_contract::PluginRequestContext;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

const MAX_CALLS: u64 = 32;
fn same_endpoint<T>(expected: &Arc<T>, actual: &Arc<T>) -> bool {
    Arc::ptr_eq(expected, actual)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CallSessionError {
    Invalid,
    Expired,
    Busy,
    Stopped,
    Unavailable,
}
use CallSessionError as E;

type Result<T> = std::result::Result<T, E>;

pub(crate) struct CallSession {
    gate: Arc<PluginDataCallGate>,
    lane: Mutex<Lane>,
}
#[derive(Default)]
struct Lane {
    stopped: bool,
    current: Option<Arc<Epoch>>,
}
struct Epoch {
    identity: PluginDataCallIdentity,
    context: PluginRequestContext,
    deadline: Instant,
    state: Mutex<EpochState>,
    changed: Condvar,
}
struct EpochState {
    live: bool,
    next_call: u64,
    in_flight: usize,
    prepared: Option<Arc<()>>,
}

// No serialization/from-wire constructor: the transport captures this token at
// spawn/connect. Equal numeric identities in two lanes do not confer authority.
#[derive(Clone)]
pub(crate) struct CallEndpoint {
    epoch: Arc<Epoch>,
}
pub(crate) struct SessionDrain {
    epoch: Arc<Epoch>,
}
pub(crate) struct CallPermit {
    epoch: Arc<Epoch>,
    _gate: PluginDataCallLease,
}
pub(crate) struct ReplyPermit {
    _call: CallPermit,
    _gate: PluginDataCallLease,
}
pub(crate) struct CompletionTicket {
    epoch: Arc<Epoch>,
    seal: Arc<()>,
}
struct SubmissionPermit {
    epoch: Arc<Epoch>,
    _gate: PluginDataCallLease,
}

impl CallSession {
    pub(crate) fn new(gate: Arc<PluginDataCallGate>) -> Self {
        Self {
            gate,
            lane: Mutex::new(Lane::default()),
        }
    }
    /// Caller has activated the real data gate. Replacement retires the previous
    /// owner immediately, but returned drain must still be awaited where shared
    /// mutable resources require old work to finish before opening a new grant.
    pub(crate) fn open(
        &self,
        context: PluginRequestContext,
        activation: u64,
        admission: u64,
        deadline: Instant,
    ) -> Result<(CallEndpoint, Option<SessionDrain>)> {
        if !context.valid_shape() {
            return Err(E::Invalid);
        }
        let identity = PluginDataCallIdentity::new(
            &context.plugin_id,
            context.plugin_generation,
            activation,
            admission,
        )
        .map_err(|_| E::Invalid)?;
        let mut lane = self.lane.lock().map_err(|_| E::Unavailable)?;
        if lane.stopped {
            return Err(E::Stopped);
        }
        let _gate = self.gate.try_acquire(&identity).map_err(|_| E::Expired)?;
        if Instant::now() >= deadline {
            return Err(E::Expired);
        }
        let old = lane
            .current
            .as_ref()
            .map(|epoch| retire(epoch))
            .transpose()?;
        let epoch = Arc::new(Epoch {
            identity,
            context,
            deadline,
            state: Mutex::new(EpochState {
                live: true,
                next_call: 1,
                in_flight: 0,
                prepared: None,
            }),
            changed: Condvar::new(),
        });
        lane.current = Some(epoch.clone());
        Ok((CallEndpoint { epoch }, old))
    }
    pub(crate) fn begin_call(
        &self,
        endpoint: &CallEndpoint,
        context: &PluginRequestContext,
        call_id: u64,
    ) -> Result<CallPermit> {
        let lane = self.lane.lock().map_err(|_| E::Unavailable)?;
        current(&lane, endpoint)?;
        if context != &endpoint.epoch.context {
            return Err(E::Expired);
        }
        let mut state = endpoint.epoch.state.lock().map_err(|_| E::Unavailable)?;
        if !state.live {
            return Err(E::Expired);
        }
        if call_id != state.next_call || call_id > MAX_CALLS {
            return Err(E::Invalid);
        }
        let gate = self
            .gate
            .try_acquire(&endpoint.epoch.identity)
            .map_err(|_| E::Expired)?;
        if Instant::now() >= endpoint.epoch.deadline {
            return Err(E::Expired);
        }
        state.next_call += 1;
        state.in_flight = state.in_flight.checked_add(1).ok_or(E::Unavailable)?;
        Ok(CallPermit {
            epoch: endpoint.epoch.clone(),
            _gate: gate,
        })
    }
    /// The call permit stays counted while its response is queued. New reply
    /// admission rechecks the activation gate; old backend admission is not a
    /// license to send after revocation. Perform I/O outside these short locks.
    pub(crate) fn begin_reply(
        &self,
        endpoint: &CallEndpoint,
        call: CallPermit,
    ) -> Result<ReplyPermit> {
        let lane = self.lane.lock().map_err(|_| E::Unavailable)?;
        current(&lane, endpoint)?;
        if !same_endpoint(&endpoint.epoch, &call.epoch) {
            return Err(E::Expired);
        }
        let state = endpoint.epoch.state.lock().map_err(|_| E::Unavailable)?;
        if !state.live {
            return Err(E::Expired);
        }
        let gate = self
            .gate
            .try_acquire(&endpoint.epoch.identity)
            .map_err(|_| E::Expired)?;
        if Instant::now() >= endpoint.epoch.deadline {
            return Err(E::Expired);
        }
        Ok(ReplyPermit {
            _call: call,
            _gate: gate,
        })
    }
    pub(crate) fn prepare_completion(
        &self,
        endpoint: &CallEndpoint,
        context: &PluginRequestContext,
        response_request_id: &str,
    ) -> Result<CompletionTicket> {
        let lane = self.lane.lock().map_err(|_| E::Unavailable)?;
        current(&lane, endpoint)?;
        if context != &endpoint.epoch.context || response_request_id != context.request_id {
            return Err(E::Expired);
        }
        let mut state = endpoint.epoch.state.lock().map_err(|_| E::Unavailable)?;
        if !state.live {
            return Err(E::Expired);
        }
        if state.in_flight != 0 {
            return Err(E::Busy);
        }
        let _gate = self
            .gate
            .try_acquire(&endpoint.epoch.identity)
            .map_err(|_| E::Expired)?;
        if Instant::now() >= endpoint.epoch.deadline {
            return Err(E::Expired);
        }
        let seal = Arc::new(());
        state.prepared = Some(seal.clone());
        state.live = false;
        Ok(CompletionTicket {
            epoch: endpoint.epoch.clone(),
            seal,
        })
    }
    /// Adapter proves its transport completion separately (e.g. normal child
    /// exit); final host acceptance runs synchronously outside state locks, but
    /// under a fresh gate lease and drain-counted permit. No cached accepted bool.
    pub(crate) fn submit_completion<T>(
        &self,
        endpoint: &CallEndpoint,
        ticket: CompletionTicket,
        accept: impl FnOnce() -> T,
    ) -> Result<T> {
        let permit = {
            let lane = self.lane.lock().map_err(|_| E::Unavailable)?;
            current(&lane, endpoint)?;
            if !same_endpoint(&endpoint.epoch, &ticket.epoch) {
                return Err(E::Expired);
            }
            let mut state = endpoint.epoch.state.lock().map_err(|_| E::Unavailable)?;
            if !state
                .prepared
                .as_ref()
                .is_some_and(|seal| same_endpoint(seal, &ticket.seal))
            {
                return Err(E::Expired);
            }
            let gate = self
                .gate
                .try_acquire(&endpoint.epoch.identity)
                .map_err(|_| E::Expired)?;
            if Instant::now() >= endpoint.epoch.deadline {
                return Err(E::Expired);
            }
            state.prepared = None;
            state.in_flight = state.in_flight.checked_add(1).ok_or(E::Unavailable)?;
            SubmissionPermit {
                epoch: endpoint.epoch.clone(),
                _gate: gate,
            }
        };
        let result = accept();
        drop(permit);
        Ok(result)
    }
    pub(crate) fn close(&self, endpoint: &CallEndpoint) -> Result<SessionDrain> {
        let mut lane = self.lane.lock().map_err(|_| E::Unavailable)?;
        current(&lane, endpoint)?;
        let drain = retire(&endpoint.epoch)?;
        lane.current = None;
        Ok(drain)
    }
    pub(crate) fn shutdown(&self) -> Result<Option<SessionDrain>> {
        let mut lane = self.lane.lock().map_err(|_| E::Unavailable)?;
        lane.stopped = true;
        let drain = lane
            .current
            .as_ref()
            .map(|epoch| retire(epoch))
            .transpose()?;
        lane.current = None;
        Ok(drain)
    }
}
fn current(lane: &Lane, endpoint: &CallEndpoint) -> Result<()> {
    if lane.stopped {
        return Err(E::Stopped);
    }
    if lane
        .current
        .as_ref()
        .is_some_and(|epoch| same_endpoint(epoch, &endpoint.epoch))
    {
        Ok(())
    } else {
        Err(E::Expired)
    }
}
fn retire(epoch: &Arc<Epoch>) -> Result<SessionDrain> {
    let mut state = epoch.state.lock().map_err(|_| E::Unavailable)?;
    state.live = false;
    state.prepared = None;
    Ok(SessionDrain {
        epoch: epoch.clone(),
    })
}
impl SessionDrain {
    /// Call only outside backend/reply/submission work and host locks. For a
    /// blocked transport, cancel/close/kill before waiting; this is not OS cancel.
    pub(crate) fn wait(self) -> Result<()> {
        let mut state = self.epoch.state.lock().map_err(|_| E::Unavailable)?;
        while state.in_flight != 0 {
            state = self.epoch.changed.wait(state).map_err(|_| E::Unavailable)?;
        }
        Ok(())
    }
}
fn released(epoch: &Epoch) {
    if let Ok(mut state) = epoch.state.lock() {
        state.in_flight -= 1;
        if state.in_flight == 0 {
            epoch.changed.notify_all();
        }
    }
}
impl Drop for CallPermit {
    fn drop(&mut self) {
        released(&self.epoch);
    }
}
impl Drop for SubmissionPermit {
    fn drop(&mut self) {
        released(&self.epoch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, thread, time::Duration};
    fn context() -> PluginRequestContext {
        PluginRequestContext {
            plugin_id: "com.uipilot.fixture".into(),
            plugin_generation: 1,
            request_id: "public-request-0000000000000001".into(),
        }
    }
    fn fixture() -> (Arc<PluginDataCallGate>, Arc<CallSession>, CallEndpoint) {
        let gate = Arc::new(PluginDataCallGate::default());
        gate.activate(identity()).unwrap();
        let session = Arc::new(CallSession::new(gate.clone()));
        let (endpoint, _) = session
            .open(context(), 1, 1, Instant::now() + Duration::from_secs(10))
            .unwrap();
        (gate, session, endpoint)
    }
    fn identity() -> PluginDataCallIdentity {
        PluginDataCallIdentity::new("com.uipilot.fixture", 1, 1, 1).unwrap()
    }
    #[test]
    fn equal_wire_values_do_not_make_two_host_endpoints_identical() {
        let (_, first, a) = fixture();
        let (_, second, b) = fixture();
        assert!(first.begin_call(&b, &context(), 1).is_err());
        assert!(second.begin_call(&a, &context(), 1).is_err());
        let ticket = first
            .prepare_completion(&a, &context(), &context().request_id)
            .unwrap();
        assert_eq!(
            second.submit_completion(&b, ticket, || panic!("foreign completion")),
            Err(E::Expired)
        );
    }
    #[test]
    fn call_ids_are_single_use_bounded_and_wrong_context_has_no_admission() {
        let (_, session, endpoint) = fixture();
        let mut wrong = context();
        wrong.request_id = "public-request-0000000000000002".into();
        assert!(session.begin_call(&endpoint, &wrong, 1).is_err());
        for id in 1..=32 {
            drop(session.begin_call(&endpoint, &context(), id).unwrap());
        }
        assert!(session.begin_call(&endpoint, &context(), 32).is_err());
        assert!(session.begin_call(&endpoint, &context(), 33).is_err());
    }
    #[test]
    fn pending_reply_blocks_completion_and_gate_close_blocks_reply_and_commit() {
        let (gate, session, endpoint) = fixture();
        let call = session.begin_call(&endpoint, &context(), 1).unwrap();
        assert!(matches!(
            session.prepare_completion(&endpoint, &context(), &context().request_id),
            Err(E::Busy)
        ));
        let gate_drain = gate.close(&identity()).unwrap();
        assert!(session.begin_reply(&endpoint, call).is_err());
        gate_drain.wait().unwrap();
        let (gate, session, endpoint) = fixture();
        let ticket = session
            .prepare_completion(&endpoint, &context(), &context().request_id)
            .unwrap();
        gate.close(&identity()).unwrap().wait().unwrap();
        assert_eq!(
            session.submit_completion(&endpoint, ticket, || panic!("closed gate")),
            Err(E::Expired)
        );
    }
    #[test]
    fn close_rejects_before_drain_and_stale_close_cannot_touch_replacement() {
        let (_, session, endpoint) = fixture();
        let call = session.begin_call(&endpoint, &context(), 1).unwrap();
        let drain = session.close(&endpoint).unwrap();
        assert!(session.begin_call(&endpoint, &context(), 2).is_err());
        let (tx, rx) = mpsc::channel();
        let waiter = thread::spawn(move || tx.send(drain.wait()).unwrap());
        assert!(rx.recv_timeout(Duration::from_millis(25)).is_err());
        drop(call);
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)), Ok(Ok(())));
        waiter.join().unwrap();
        let (fresh, _) = session
            .open(context(), 1, 1, Instant::now() + Duration::from_secs(10))
            .unwrap();
        assert!(session.close(&endpoint).is_err());
        assert!(session.begin_call(&fresh, &context(), 1).is_ok());
    }
    #[test]
    fn prepared_completion_is_single_use_and_close_or_replacement_retires_it() {
        let (_, session, endpoint) = fixture();
        let ticket = session
            .prepare_completion(&endpoint, &context(), &context().request_id)
            .unwrap();
        assert!(session
            .prepare_completion(&endpoint, &context(), &context().request_id)
            .is_err());
        assert!(session.begin_call(&endpoint, &context(), 1).is_err());
        session.close(&endpoint).unwrap().wait().unwrap();
        assert_eq!(
            session.submit_completion(&endpoint, ticket, || panic!("retired")),
            Err(E::Expired)
        );
    }
    #[test]
    fn submission_is_outside_locks_and_drain_covers_actual_host_acceptance() {
        let (_, session, endpoint) = fixture();
        let ticket = session
            .prepare_completion(&endpoint, &context(), &context().request_id)
            .unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker_session = session.clone();
        let worker_endpoint = endpoint.clone();
        let worker = thread::spawn(move || {
            worker_session.submit_completion(&worker_endpoint, ticket, || {
                started_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                7
            })
        });
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let drain = session.close(&endpoint).unwrap();
        assert!(session.begin_call(&endpoint, &context(), 1).is_err());
        let (tx, rx) = mpsc::channel();
        let waiter = thread::spawn(move || tx.send(drain.wait()).unwrap());
        assert!(rx.recv_timeout(Duration::from_millis(25)).is_err());
        release_tx.send(()).unwrap();
        assert_eq!(worker.join().unwrap(), Ok(7));
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)), Ok(Ok(())));
        waiter.join().unwrap();
    }
    #[test]
    fn expired_deadline_and_permanent_shutdown_fail_closed() {
        let (_, session, endpoint) = fixture();
        assert!(session
            .open(context(), 1, 1, Instant::now() - Duration::from_secs(1))
            .is_err());
        session.shutdown().unwrap().unwrap().wait().unwrap();
        assert!(session.begin_call(&endpoint, &context(), 1).is_err());
        assert!(matches!(
            session.open(context(), 1, 1, Instant::now() + Duration::from_secs(1)),
            Err(E::Stopped)
        ));
    }
    #[test]
    fn admitted_reply_and_completion_replacement_and_expiry_are_checked() {
        let (_, session, endpoint) = fixture();
        let call = session.begin_call(&endpoint, &context(), 1).unwrap();
        let reply = session.begin_reply(&endpoint, call).unwrap();
        assert!(matches!(
            session.prepare_completion(&endpoint, &context(), &context().request_id),
            Err(E::Busy)
        ));
        drop(reply);
        let ticket = session
            .prepare_completion(&endpoint, &context(), &context().request_id)
            .unwrap();
        let (fresh, drain) = session
            .open(context(), 1, 1, Instant::now() + Duration::from_secs(1))
            .unwrap();
        drain.unwrap().wait().unwrap();
        assert_eq!(
            session.submit_completion(&endpoint, ticket, || panic!("old publication")),
            Err(E::Expired)
        );
        assert!(session.begin_call(&fresh, &context(), 1).is_ok());
        let (short, old) = session
            .open(context(), 1, 1, Instant::now() + Duration::from_millis(25))
            .unwrap();
        old.unwrap().wait().unwrap();
        let ticket = session
            .prepare_completion(&short, &context(), &context().request_id)
            .unwrap();
        thread::sleep(Duration::from_millis(30));
        assert_eq!(
            session.submit_completion(&short, ticket, || panic!("expired publication")),
            Err(E::Expired)
        );
    }
    #[test]
    fn already_admitted_submission_may_finish_after_replacement_but_is_drained() {
        let (_, session, endpoint) = fixture();
        let ticket = session
            .prepare_completion(&endpoint, &context(), &context().request_id)
            .unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker_session = session.clone();
        let old = endpoint.clone();
        let worker = thread::spawn(move || {
            worker_session.submit_completion(&old, ticket, || {
                started_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            })
        });
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let (fresh, drain) = session
            .open(context(), 1, 1, Instant::now() + Duration::from_secs(10))
            .unwrap();
        assert!(session.close(&endpoint).is_err());
        assert!(session.begin_call(&fresh, &context(), 1).is_ok());
        let (tx, rx) = mpsc::channel();
        let waiter = thread::spawn(move || tx.send(drain.unwrap().wait()).unwrap());
        assert!(rx.recv_timeout(Duration::from_millis(25)).is_err());
        release_tx.send(()).unwrap();
        worker.join().unwrap().unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)), Ok(Ok(())));
        waiter.join().unwrap();
    }
}
