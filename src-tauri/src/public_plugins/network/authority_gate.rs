use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex, MutexGuard},
};

use super::{
    super::{
        manifest::valid_https_host, scheduler::PluginContextAccessError, PluginRequestContext,
        PluginRequestScheduler,
    },
    broker::{
        PendingPluginNetworkResponse, PluginHttpsBroker, PluginNetworkErrorCode,
        PluginNetworkRequest, PluginNetworkResponse, PreparedPluginNetworkCall, RedirectAuthority,
    },
    registry::{PluginNetworkCallIdentity, PluginNetworkContextIdentity},
};

#[cfg(test)]
use super::{
    registry::PluginNetworkRequestRegistry,
    transport::{
        BoundedDnsResolver, HttpsTransport, HttpsTransportFuture, NativeHttpsRequest,
        NativeTransportError,
    },
};

#[cfg(test)]
struct LifecycleBlockingTransport {
    started: Arc<tokio::sync::Notify>,
}

#[cfg(test)]
impl HttpsTransport for LifecycleBlockingTransport {
    fn execute<'a>(
        &'a self,
        _request: NativeHttpsRequest,
        cancellation: &'a tokio_util::sync::CancellationToken,
    ) -> HttpsTransportFuture<'a> {
        Box::pin(async move {
            self.started.notify_one();
            cancellation.cancelled().await;
            Err(NativeTransportError::Cancelled)
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PluginNetworkAuthoritySnapshot {
    pub(crate) plugin_id: String,
    pub(crate) plugin_generation: u64,
    pub(crate) activation_id: u64,
    pub(crate) admission_epoch: u64,
    pub(crate) https_hosts: BTreeSet<String>,
}

impl PluginNetworkAuthoritySnapshot {
    pub(crate) fn new(
        plugin_id: &str,
        plugin_generation: u64,
        activation_id: u64,
        admission_epoch: u64,
        https_hosts: BTreeSet<String>,
    ) -> Result<Self, PluginNetworkErrorCode> {
        if !super::super::manifest::valid_plugin_id(plugin_id)
            || plugin_generation == 0
            || activation_id == 0
            || admission_epoch == 0
            || https_hosts.is_empty()
            || https_hosts.len() > 8
            || https_hosts.iter().any(|host| !valid_https_host(host))
        {
            return Err(PluginNetworkErrorCode::NetworkFailure);
        }
        Ok(Self {
            plugin_id: plugin_id.to_owned(),
            plugin_generation,
            activation_id,
            admission_epoch,
            https_hosts,
        })
    }

    fn matches_call(&self, identity: &PluginNetworkCallIdentity) -> bool {
        self.plugin_id == identity.context.plugin_id
            && self.plugin_generation == identity.context.plugin_generation
            && self.activation_id == identity.context.activation_id
            && self.admission_epoch == identity.context.admission_epoch
    }

    fn retained_identity(&self) -> (u64, u64, u64) {
        (
            self.plugin_generation,
            self.activation_id,
            self.admission_epoch,
        )
    }
}

#[derive(Default)]
struct AuthorityState {
    by_plugin: HashMap<String, PluginNetworkAuthoritySnapshot>,
    terminal: bool,
}

pub(crate) struct PluginNetworkAuthorityGate {
    state: Mutex<AuthorityState>,
    broker: Arc<PluginHttpsBroker>,
}

pub(crate) struct PluginNetworkAuthorityTransition<'a> {
    state: MutexGuard<'a, AuthorityState>,
    broker: &'a PluginHttpsBroker,
}

impl PluginNetworkAuthorityGate {
    pub(crate) fn new() -> Result<Self, PluginNetworkErrorCode> {
        Ok(Self::with_broker(Arc::new(PluginHttpsBroker::new()?)))
    }

    pub(super) fn with_broker(broker: Arc<PluginHttpsBroker>) -> Self {
        Self {
            state: Mutex::new(AuthorityState::default()),
            broker,
        }
    }

    pub(super) fn broker(&self) -> &Arc<PluginHttpsBroker> {
        &self.broker
    }

    pub(crate) fn publish(
        &self,
        snapshot: Option<PluginNetworkAuthoritySnapshot>,
    ) -> Result<(), PluginNetworkErrorCode> {
        let snapshot = snapshot.ok_or(PluginNetworkErrorCode::NetworkFailure)?;
        let plugin_id = snapshot.plugin_id.clone();
        self.publish_plugin(&plugin_id, Some(snapshot))
    }

    pub(crate) fn publish_plugin(
        &self,
        plugin_id: &str,
        snapshot: Option<PluginNetworkAuthoritySnapshot>,
    ) -> Result<(), PluginNetworkErrorCode> {
        self.begin_transition()?
            .set_plugin_authority(plugin_id, snapshot)
    }

    pub(crate) fn begin_transition(
        &self,
    ) -> Result<PluginNetworkAuthorityTransition<'_>, PluginNetworkErrorCode> {
        let state = self.lock()?;
        if state.terminal {
            return Err(PluginNetworkErrorCode::NetworkFailure);
        }
        Ok(PluginNetworkAuthorityTransition {
            state,
            broker: self.broker.as_ref(),
        })
    }

    pub(crate) fn admit(
        &self,
        caller_plugin_id: &str,
        caller_generation: u64,
        context: &PluginRequestContext,
        scheduler: &PluginRequestScheduler,
        request: PluginNetworkRequest,
    ) -> Result<PreparedPluginNetworkCall, PluginNetworkErrorCode> {
        self.admit_inner(
            caller_plugin_id,
            caller_generation,
            context,
            scheduler,
            request,
            || {},
        )
    }

    fn admit_inner(
        &self,
        caller_plugin_id: &str,
        caller_generation: u64,
        context: &PluginRequestContext,
        scheduler: &PluginRequestScheduler,
        request: PluginNetworkRequest,
        hook: impl FnOnce(),
    ) -> Result<PreparedPluginNetworkCall, PluginNetworkErrorCode> {
        let state = self.lock()?;
        if state.terminal
            || caller_plugin_id != context.plugin_id
            || caller_generation != context.plugin_generation
        {
            return Err(PluginNetworkErrorCode::ExpiredRequest);
        }
        let current = current_context_identity(context, scheduler)?;
        let authority = state
            .by_plugin
            .get(caller_plugin_id)
            .filter(|authority| {
                authority.plugin_generation == current.plugin_generation
                    && authority.activation_id == current.activation_id
                    && authority.admission_epoch == current.admission_epoch
            })
            .ok_or(PluginNetworkErrorCode::PermissionDenied)?;
        hook();
        self.broker.admit(&current, &authority.https_hosts, request)
    }

    #[cfg(test)]
    fn admit_with_hook(
        &self,
        caller_plugin_id: &str,
        caller_generation: u64,
        context: &PluginRequestContext,
        scheduler: &PluginRequestScheduler,
        request: PluginNetworkRequest,
        hook: impl FnOnce(),
    ) -> Result<PreparedPluginNetworkCall, PluginNetworkErrorCode> {
        self.admit_inner(
            caller_plugin_id,
            caller_generation,
            context,
            scheduler,
            request,
            hook,
        )
    }

    pub(crate) fn redirect_authority(
        self: &Arc<Self>,
        scheduler: Arc<PluginRequestScheduler>,
    ) -> RedirectAuthority {
        let gate = self.clone();
        Arc::new(move |identity, hostname| gate.call_is_current(identity, hostname, &scheduler))
    }

    pub(crate) fn deliver(
        &self,
        pending: PendingPluginNetworkResponse,
        scheduler: &PluginRequestScheduler,
    ) -> Result<PluginNetworkResponse, PluginNetworkErrorCode> {
        let state = self.lock()?;
        if !call_is_current_locked(&state, pending.identity(), scheduler, None) {
            return Err(PluginNetworkErrorCode::ExpiredRequest);
        }
        pending.deliver()
    }

    pub(crate) fn invalidate_context(&self, context: &PluginRequestContext) -> usize {
        self.begin_transition()
            .map_or(0, |transition| transition.invalidate_context(context))
    }

    pub(crate) fn invalidate_runtime(
        &self,
        plugin_id: &str,
        plugin_generation: u64,
        activation_id: u64,
        admission_epoch: u64,
    ) -> usize {
        self.begin_transition().map_or(0, |mut transition| {
            transition.invalidate_runtime(
                plugin_id,
                plugin_generation,
                activation_id,
                admission_epoch,
            )
        })
    }

    pub(crate) fn shutdown(&self) -> usize {
        let Ok(mut state) = self.lock() else {
            return 0;
        };
        state.terminal = true;
        state.by_plugin.clear();
        self.broker.shutdown()
    }

    pub(crate) fn close_plugin(&self, plugin_id: &str) -> usize {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.by_plugin.remove(plugin_id);
        self.broker.publish_plugin_authority(plugin_id, None)
    }

    #[cfg(test)]
    pub(crate) fn blocking_for_test() -> (Arc<Self>, Arc<tokio::sync::Notify>) {
        let started = Arc::new(tokio::sync::Notify::new());
        let resolver = BoundedDnsResolver::with_lookup(1, 2, |_host, port| {
            Ok(vec![std::net::SocketAddr::from(([8, 8, 8, 8], port))])
        });
        let broker = Arc::new(PluginHttpsBroker::with_dependencies(
            PluginNetworkRequestRegistry::default(),
            resolver,
            Arc::new(LifecycleBlockingTransport {
                started: Arc::clone(&started),
            }),
        ));
        (Arc::new(Self::with_broker(broker)), started)
    }

    #[cfg(test)]
    pub(crate) async fn execute_for_test(
        self: &Arc<Self>,
        prepared: PreparedPluginNetworkCall,
        scheduler: Arc<PluginRequestScheduler>,
    ) -> Result<PluginNetworkResponse, PluginNetworkErrorCode> {
        let pending = self
            .broker
            .execute(prepared, self.redirect_authority(Arc::clone(&scheduler)))
            .await?;
        self.deliver(pending, &scheduler)
    }

    #[cfg(test)]
    pub(crate) fn global_active_for_test(&self) -> usize {
        self.broker.global_active_for_test()
    }

    fn call_is_current(
        &self,
        identity: &PluginNetworkCallIdentity,
        hostname: &str,
        scheduler: &PluginRequestScheduler,
    ) -> bool {
        let Ok(state) = self.lock() else {
            return false;
        };
        call_is_current_locked(&state, identity, scheduler, Some(hostname))
    }

    fn lock(&self) -> Result<MutexGuard<'_, AuthorityState>, PluginNetworkErrorCode> {
        self.state
            .lock()
            .map_err(|_| PluginNetworkErrorCode::NetworkFailure)
    }
}

impl PluginNetworkAuthorityTransition<'_> {
    pub(crate) fn set_plugin_authority(
        &mut self,
        plugin_id: &str,
        snapshot: Option<PluginNetworkAuthoritySnapshot>,
    ) -> Result<(), PluginNetworkErrorCode> {
        if snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.plugin_id != plugin_id)
        {
            return Err(PluginNetworkErrorCode::NetworkFailure);
        }
        let retained = snapshot
            .as_ref()
            .map(PluginNetworkAuthoritySnapshot::retained_identity);
        if let Some(snapshot) = snapshot {
            self.state.by_plugin.insert(plugin_id.to_owned(), snapshot);
        } else {
            self.state.by_plugin.remove(plugin_id);
        }
        self.broker.publish_plugin_authority(plugin_id, retained);
        Ok(())
    }

    pub(crate) fn invalidate_context(&self, context: &PluginRequestContext) -> usize {
        self.broker.cancel_request_context(
            &context.plugin_id,
            context.plugin_generation,
            &context.request_id,
        )
    }

    pub(crate) fn invalidate_runtime(
        &mut self,
        plugin_id: &str,
        plugin_generation: u64,
        activation_id: u64,
        admission_epoch: u64,
    ) -> usize {
        if self
            .state
            .by_plugin
            .get(plugin_id)
            .is_some_and(|authority| {
                authority.plugin_generation == plugin_generation
                    && authority.activation_id == activation_id
                    && authority.admission_epoch == admission_epoch
            })
        {
            self.state.by_plugin.remove(plugin_id);
        }
        self.broker
            .cancel_runtime(plugin_id, plugin_generation, activation_id, admission_epoch)
    }

    pub(crate) fn invalidate_generation(
        &mut self,
        plugin_id: &str,
        plugin_generation: u64,
    ) -> usize {
        if self
            .state
            .by_plugin
            .get(plugin_id)
            .is_some_and(|authority| authority.plugin_generation == plugin_generation)
        {
            self.state.by_plugin.remove(plugin_id);
        }
        self.broker.cancel_generation(plugin_id, plugin_generation)
    }
}

fn current_context_identity(
    context: &PluginRequestContext,
    scheduler: &PluginRequestScheduler,
) -> Result<PluginNetworkContextIdentity, PluginNetworkErrorCode> {
    scheduler
        .with_current(context, |current| {
            PluginNetworkContextIdentity::new(
                &context.plugin_id,
                current.plugin_generation(),
                current.activation_id(),
                current.admission_epoch(),
                &context.request_id,
            )
        })
        .map_err(map_context_error)?
        .map_err(|_| PluginNetworkErrorCode::NetworkFailure)
}

fn call_is_current_locked(
    state: &AuthorityState,
    identity: &PluginNetworkCallIdentity,
    scheduler: &PluginRequestScheduler,
    hostname: Option<&str>,
) -> bool {
    if state.terminal {
        return false;
    }
    let context = PluginRequestContext {
        plugin_id: identity.context.plugin_id.clone(),
        plugin_generation: identity.context.plugin_generation,
        request_id: identity.context.request_id.clone(),
    };
    let Ok(current) = current_context_identity(&context, scheduler) else {
        return false;
    };
    current == identity.context
        && state
            .by_plugin
            .get(&identity.context.plugin_id)
            .is_some_and(|authority| {
                authority.matches_call(identity)
                    && hostname.is_none_or(|hostname| authority.https_hosts.contains(hostname))
            })
}

fn map_context_error(error: PluginContextAccessError) -> PluginNetworkErrorCode {
    match error {
        PluginContextAccessError::Expired | PluginContextAccessError::Invalid => {
            PluginNetworkErrorCode::ExpiredRequest
        }
        PluginContextAccessError::Unavailable => PluginNetworkErrorCode::NetworkFailure,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        net::SocketAddr,
        sync::{mpsc, Arc},
        thread,
        time::{Duration, Instant},
    };

    use tokio::sync::Notify;
    use tokio_util::sync::CancellationToken;

    use super::{PluginNetworkAuthorityGate, PluginNetworkAuthoritySnapshot};
    use crate::public_plugins::{
        manifest::PublicActivationMode,
        network::{
            broker::{
                PluginHttpsBroker, PluginNetworkErrorCode, PluginNetworkRequest,
                PluginNetworkRequestMethod,
            },
            registry::PluginNetworkRequestRegistry,
            transport::{
                BoundedDnsResolver, DeterministicHttpsTransport, HttpsTransport,
                HttpsTransportFuture, NativeHttpsRequest, NativeHttpsResponse,
                NativeTransportError,
            },
        },
        PluginRequestCandidate, PluginRequestContext, PluginRequestScheduler,
        PluginScheduleOutcome, PluginSubmissionOwner,
    };

    struct BlockingTransport {
        started: Arc<Notify>,
    }

    impl HttpsTransport for BlockingTransport {
        fn execute<'a>(
            &'a self,
            _request: NativeHttpsRequest,
            cancellation: &'a CancellationToken,
        ) -> HttpsTransportFuture<'a> {
            Box::pin(async move {
                self.started.notify_one();
                cancellation.cancelled().await;
                Err(NativeTransportError::Cancelled)
            })
        }
    }

    fn scheduler(
        generation: u64,
        activation_id: u64,
        admission_epoch: u64,
    ) -> (Arc<PluginRequestScheduler>, PluginRequestContext) {
        let scheduler = Arc::new(PluginRequestScheduler::default());
        let outcome = scheduler
            .enqueue(
                PluginRequestCandidate {
                    plugin_id: "com.example.network".into(),
                    plugin_generation: generation,
                    activation_id,
                    admission_epoch,
                    activation_mode: PublicActivationMode::Live,
                    input: String::new(),
                    owner: PluginSubmissionOwner {
                        ui_intent_epoch: 1,
                        control_value: String::new(),
                        submission_token: format!("submission-{generation}"),
                    },
                },
                Instant::now(),
            )
            .unwrap();
        let PluginScheduleOutcome::Dispatched(request) = outcome else {
            panic!("expected dispatched request");
        };
        (scheduler, request.context)
    }

    fn snapshot(
        generation: u64,
        activation_id: u64,
        admission_epoch: u64,
    ) -> PluginNetworkAuthoritySnapshot {
        PluginNetworkAuthoritySnapshot::new(
            "com.example.network",
            generation,
            activation_id,
            admission_epoch,
            BTreeSet::from(["api.example.com".into()]),
        )
        .unwrap()
    }

    fn request() -> PluginNetworkRequest {
        PluginNetworkRequest {
            url: "https://api.example.com/data".into(),
            method: PluginNetworkRequestMethod::Get,
            headers: None,
            body: None,
        }
    }

    fn resolver() -> BoundedDnsResolver {
        BoundedDnsResolver::with_lookup(1, 2, |_host, port| {
            Ok(vec![SocketAddr::from(([8, 8, 8, 8], port))])
        })
    }

    #[test]
    fn plugin_network_authority_transition_cannot_slip_between_validation_and_registration() {
        let broker = Arc::new(PluginHttpsBroker::with_dependencies(
            PluginNetworkRequestRegistry::default(),
            resolver(),
            Arc::new(DeterministicHttpsTransport::new(Vec::new())),
        ));
        let gate = Arc::new(PluginNetworkAuthorityGate::with_broker(broker));
        let (scheduler, context) = scheduler(1, 2, 3);
        gate.publish(Some(snapshot(1, 2, 3))).unwrap();
        let (entered, entered_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let admission = thread::spawn({
            let gate = gate.clone();
            let scheduler = scheduler.clone();
            let context = context.clone();
            move || {
                gate.admit_with_hook(
                    "com.example.network",
                    1,
                    &context,
                    &scheduler,
                    request(),
                    || {
                        entered.send(()).unwrap();
                        release_rx.recv().unwrap();
                    },
                )
            }
        });
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let (transitioned, transitioned_rx) = mpsc::channel();
        let transition = thread::spawn({
            let gate = gate.clone();
            move || {
                gate.publish_plugin("com.example.network", None).unwrap();
                transitioned.send(()).unwrap();
            }
        });
        assert!(transitioned_rx
            .recv_timeout(Duration::from_millis(50))
            .is_err());
        release.send(()).unwrap();
        let prepared = admission.join().unwrap().unwrap();
        transition.join().unwrap();
        assert_eq!(transitioned_rx.recv_timeout(Duration::from_secs(1)), Ok(()));
        drop(prepared);
    }

    #[tokio::test]
    async fn plugin_network_authority_revoke_between_response_and_delivery_expires_response() {
        let broker = Arc::new(PluginHttpsBroker::with_dependencies(
            PluginNetworkRequestRegistry::default(),
            resolver(),
            Arc::new(DeterministicHttpsTransport::new(vec![Ok(
                NativeHttpsResponse {
                    status: 200,
                    headers: Vec::new(),
                    body: b"ok".to_vec(),
                },
            )])),
        ));
        let gate = Arc::new(PluginNetworkAuthorityGate::with_broker(broker.clone()));
        let (scheduler, context) = scheduler(1, 2, 3);
        gate.publish(Some(snapshot(1, 2, 3))).unwrap();
        let prepared = gate
            .admit("com.example.network", 1, &context, &scheduler, request())
            .unwrap();
        let pending = broker
            .execute(prepared, gate.redirect_authority(scheduler.clone()))
            .await
            .unwrap();
        gate.publish_plugin("com.example.network", None).unwrap();
        assert_eq!(
            gate.deliver(pending, &scheduler),
            Err(PluginNetworkErrorCode::ExpiredRequest)
        );
    }

    #[tokio::test]
    async fn plugin_network_authority_replacement_between_response_and_delivery_expires_response() {
        let broker = Arc::new(PluginHttpsBroker::with_dependencies(
            PluginNetworkRequestRegistry::default(),
            resolver(),
            Arc::new(DeterministicHttpsTransport::new(vec![Ok(
                NativeHttpsResponse {
                    status: 200,
                    headers: Vec::new(),
                    body: b"ok".to_vec(),
                },
            )])),
        ));
        let gate = Arc::new(PluginNetworkAuthorityGate::with_broker(broker.clone()));
        let (scheduler, context) = scheduler(1, 2, 3);
        gate.publish(Some(snapshot(1, 2, 3))).unwrap();
        let prepared = gate
            .admit("com.example.network", 1, &context, &scheduler, request())
            .unwrap();
        let pending = broker
            .execute(prepared, gate.redirect_authority(scheduler.clone()))
            .await
            .unwrap();
        gate.publish_plugin("com.example.network", Some(snapshot(2, 4, 5)))
            .unwrap();
        assert_eq!(
            gate.deliver(pending, &scheduler),
            Err(PluginNetworkErrorCode::ExpiredRequest)
        );
    }

    #[tokio::test]
    async fn plugin_network_authority_stale_generation_cancel_cannot_touch_new_call() {
        let started = Arc::new(Notify::new());
        let broker = Arc::new(PluginHttpsBroker::with_dependencies(
            PluginNetworkRequestRegistry::default(),
            resolver(),
            Arc::new(BlockingTransport {
                started: started.clone(),
            }),
        ));
        let gate = Arc::new(PluginNetworkAuthorityGate::with_broker(broker.clone()));
        let (old_scheduler, old_context) = scheduler(1, 2, 3);
        gate.publish(Some(snapshot(1, 2, 3))).unwrap();
        let old = gate
            .admit(
                "com.example.network",
                1,
                &old_context,
                &old_scheduler,
                request(),
            )
            .unwrap();
        gate.publish_plugin("com.example.network", Some(snapshot(2, 4, 5)))
            .unwrap();
        drop(old);

        let (new_scheduler, new_context) = scheduler(2, 4, 5);
        let new_call = gate
            .admit(
                "com.example.network",
                2,
                &new_context,
                &new_scheduler,
                request(),
            )
            .unwrap();
        let pending = tokio::spawn({
            let broker = broker.clone();
            let authority = gate.redirect_authority(new_scheduler.clone());
            async move { broker.execute(new_call, authority).await }
        });
        tokio::time::timeout(Duration::from_millis(500), started.notified())
            .await
            .unwrap();
        assert_eq!(gate.invalidate_runtime("com.example.network", 1, 2, 3), 0);
        assert!(!pending.is_finished());
        assert_eq!(
            gate.begin_transition()
                .unwrap()
                .invalidate_generation("com.example.network", 1),
            0
        );
        assert!(!pending.is_finished());
        assert_eq!(gate.invalidate_context(&new_context), 1);
        assert_eq!(
            pending.await.unwrap().err(),
            Some(PluginNetworkErrorCode::ExpiredRequest)
        );
    }
}
