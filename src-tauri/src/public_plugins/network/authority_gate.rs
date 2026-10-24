use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex, MutexGuard},
};

use super::{
    super::{
        manifest::valid_https_host, scheduler::PluginContextAccessError,
        secrets::PluginSecretStore, PluginRequestContext, PluginRequestScheduler,
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

#[cfg(test)]
pub(crate) struct LifecycleNetworkBarrier {
    pub(crate) reached: tokio::sync::Notify,
    pub(crate) release: tokio::sync::Notify,
    pub(crate) sends: std::sync::atomic::AtomicUsize,
    status: u16,
}

#[cfg(test)]
impl HttpsTransport for LifecycleNetworkBarrier {
    fn execute<'a>(
        &'a self,
        _request: NativeHttpsRequest,
        _cancellation: &'a tokio_util::sync::CancellationToken,
    ) -> HttpsTransportFuture<'a> {
        Box::pin(async move {
            self.sends.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let response = super::transport::NativeHttpsResponse {
                status: self.status,
                headers: if self.status == 302 {
                    vec![("location".into(), "/redirected".into())]
                } else {
                    vec![]
                },
                body: b"synthetic-response".to_vec(),
            };
            self.reached.notify_one();
            self.release.notified().await;
            // Deliberately ignore cancellation to exercise Host redirect/delivery checks.
            Ok(response)
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
    pub(crate) credentials: Vec<super::super::manifest::PluginNetworkCredential>,
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
            credentials: Vec::new(),
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

    #[cfg(test)]
    pub(crate) fn publish(
        &self,
        snapshot: Option<PluginNetworkAuthoritySnapshot>,
    ) -> Result<(), PluginNetworkErrorCode> {
        let snapshot = snapshot.ok_or(PluginNetworkErrorCode::NetworkFailure)?;
        let plugin_id = snapshot.plugin_id.clone();
        self.publish_plugin(&plugin_id, Some(snapshot))
    }

    #[cfg(test)]
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
            None,
            || {},
        )
    }

    pub(crate) fn admit_with_secrets(
        &self,
        caller_plugin_id: &str,
        caller_generation: u64,
        context: &PluginRequestContext,
        scheduler: &PluginRequestScheduler,
        request: PluginNetworkRequest,
        secrets: &PluginSecretStore,
    ) -> Result<PreparedPluginNetworkCall, PluginNetworkErrorCode> {
        self.admit_inner(
            caller_plugin_id,
            caller_generation,
            context,
            scheduler,
            request,
            Some(secrets),
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
        secrets: Option<&PluginSecretStore>,
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
        self.broker.admit_with_credentials(
            &current,
            &authority.https_hosts,
            &authority.credentials,
            request,
            |binding| {
                secrets
                    .ok_or(PluginNetworkErrorCode::CredentialUnavailable)?
                    .read_bearer(caller_plugin_id, &binding.setting_key)
                    .map_err(|_| PluginNetworkErrorCode::CredentialUnavailable)?
                    .ok_or(PluginNetworkErrorCode::CredentialUnavailable)
            },
            hook,
        )
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
            None,
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

    pub(crate) async fn request(
        self: &Arc<Self>,
        caller_plugin_id: &str,
        caller_generation: u64,
        context: &PluginRequestContext,
        scheduler: Arc<PluginRequestScheduler>,
        request: PluginNetworkRequest,
        secrets: &PluginSecretStore,
    ) -> Result<PluginNetworkResponse, PluginNetworkErrorCode> {
        let prepared = self.admit_with_secrets(
            caller_plugin_id,
            caller_generation,
            context,
            scheduler.as_ref(),
            request,
            secrets,
        )?;
        let pending = self
            .broker
            .execute(prepared, self.redirect_authority(Arc::clone(&scheduler)))
            .await?;
        self.deliver(pending, scheduler.as_ref())
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
    pub(crate) fn paused_response_for_test(
        status: u16,
    ) -> (Arc<Self>, Arc<LifecycleNetworkBarrier>) {
        let barrier = Arc::new(LifecycleNetworkBarrier {
            reached: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
            sends: std::sync::atomic::AtomicUsize::new(0),
            status,
        });
        let resolver = BoundedDnsResolver::with_lookup(1, 2, |_host, port| {
            Ok(vec![std::net::SocketAddr::from(([8, 8, 8, 8], port))])
        });
        let broker = Arc::new(PluginHttpsBroker::with_dependencies(
            PluginNetworkRequestRegistry::default(),
            resolver,
            barrier.clone(),
        ));
        (Arc::new(Self::with_broker(broker)), barrier)
    }

    #[cfg(test)]
    pub(crate) async fn execute_before_delivery_for_test(
        self: &Arc<Self>,
        prepared: PreparedPluginNetworkCall,
        scheduler: Arc<PluginRequestScheduler>,
        before_delivery: impl FnOnce(),
    ) -> Result<PluginNetworkResponse, PluginNetworkErrorCode> {
        let pending = self
            .broker
            .execute(prepared, self.redirect_authority(scheduler.clone()))
            .await?;
        before_delivery();
        self.deliver(pending, &scheduler)
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
                    command_id: "default".into(),
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
            credential: None,
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

    struct SecretFixture {
        root: std::path::PathBuf,
        store: crate::public_plugins::secrets::PluginSecretStore,
    }

    impl SecretFixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "uipilot-credential-gate-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let store = crate::public_plugins::secrets::PluginSecretStore::load(&root).unwrap();
            Self { root, store }
        }
    }
    impl Drop for SecretFixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.root).unwrap();
        }
    }

    fn bearer_snapshot() -> PluginNetworkAuthoritySnapshot {
        use crate::public_plugins::{PublicPlatform, PublicPluginHost};
        let manifest = serde_json::json!({
            "schemaVersion":1,"pluginId":"com.example.network","version":"1.0.0","apiVersion":1,
            "minimumHostVersion":"0.4.0","name":"Credential Test","supportedPlatforms":["windows"],
            "command":{"defaultName":"credential","activationMode":"submit","outputMode":"mainResult","inputRequired":false},
            "runtime":{"entry":"dist/runtime.js"},"permissions":["network.https"],
            "settings":[{"type":"secret","key":"api-token","label":"Token"}],
            "network":{"httpsHosts":["api.example.com"],"credentials":[{"id":"service-token","settingKey":"api-token","host":"api.example.com","type":"bearer"}]}
        });
        let host = PublicPluginHost {
            version: [0, 4, 0],
            ..PublicPluginHost::current(PublicPlatform::Windows)
        };
        let manifest = crate::public_plugins::manifest::parse_manifest(
            &serde_json::to_vec(&manifest).unwrap(),
            &host,
        )
        .unwrap();
        let mut authority = snapshot(1, 2, 3);
        authority.credentials = manifest.network.unwrap().credentials.unwrap();
        authority
    }

    fn bearer_request() -> PluginNetworkRequest {
        PluginNetworkRequest {
            credential: Some("service-token".into()),
            ..request()
        }
    }

    #[tokio::test]
    async fn credential_dpapi_to_fake_transport_redacts_and_preserves_redirect_snapshot_and_echo() {
        let fixture = SecretFixture::new();
        fixture
            .store
            .write("com.example.network", "api-token", "synthetic-token-M4")
            .unwrap();
        fixture
            .store
            .write("com.example.other", "api-token", "other-synthetic-token")
            .unwrap();
        let transport = Arc::new(DeterministicHttpsTransport::new(vec![
            Ok(NativeHttpsResponse {
                status: 302,
                headers: vec![("location".into(), b"/next".to_vec())],
                body: vec![],
            }),
            Ok(NativeHttpsResponse {
                status: 200,
                headers: vec![],
                body: b"server echo: synthetic-token-M4".to_vec(),
            }),
        ]));
        let broker = Arc::new(PluginHttpsBroker::with_dependencies(
            PluginNetworkRequestRegistry::default(),
            resolver(),
            transport.clone(),
        ));
        let gate = Arc::new(PluginNetworkAuthorityGate::with_broker(broker.clone()));
        let (scheduler, context) = scheduler(1, 2, 3);
        gate.publish(Some(bearer_snapshot())).unwrap();
        let prepared = gate
            .admit_with_secrets(
                "com.example.network",
                1,
                &context,
                &scheduler,
                bearer_request(),
                &fixture.store,
            )
            .unwrap();
        // A redirect reuses its admitted snapshot even if the underlying record changes.
        // Production mutation additionally revokes this context (Task4).
        fixture
            .store
            .write(
                "com.example.network",
                "api-token",
                "updated-synthetic-token",
            )
            .unwrap();
        let pending = broker
            .execute(prepared, gate.redirect_authority(scheduler.clone()))
            .await
            .unwrap();
        let response = gate.deliver(pending, &scheduler).unwrap();
        assert_eq!(response.body, "server echo: synthetic-token-M4");
        assert!(response.headers.is_empty());
        let sent = transport.take_requests();
        assert_eq!(sent.len(), 2);
        for request in sent {
            assert!(!format!("{request:?}").contains("synthetic-token"));
            assert!(request
                .headers
                .iter()
                .all(|(name, _)| name != "authorization"));
            assert_eq!(request.sensitive_headers.len(), 1);
            let (name, value) = &request.sensitive_headers[0];
            assert_eq!(name, "authorization");
            assert!(value.is_sensitive());
            assert_eq!(value.as_bytes(), b"Bearer synthetic-token-M4");
        }
    }

    #[tokio::test]
    async fn credential_read_then_register_gate_blocks_revoke_and_cancels_old_delivery() {
        let fixture = Arc::new(SecretFixture::new());
        fixture
            .store
            .write(
                "com.example.network",
                "api-token",
                "synthetic-barrier-token",
            )
            .unwrap();
        let broker = Arc::new(PluginHttpsBroker::with_dependencies(
            PluginNetworkRequestRegistry::default(),
            resolver(),
            Arc::new(DeterministicHttpsTransport::new(vec![])),
        ));
        let gate = Arc::new(PluginNetworkAuthorityGate::with_broker(broker));
        let (scheduler, context) = scheduler(1, 2, 3);
        gate.publish(Some(bearer_snapshot())).unwrap();
        let (entered, entered_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let admission = thread::spawn({
            let gate = gate.clone();
            let scheduler = scheduler.clone();
            let context = context.clone();
            let fixture = fixture.clone();
            move || {
                gate.admit_inner(
                    "com.example.network",
                    1,
                    &context,
                    &scheduler,
                    bearer_request(),
                    Some(&fixture.store),
                    || {
                        // This hook runs only after DPAPI and sensitive header preparation, before registration.
                        assert_eq!(gate.global_active_for_test(), 0);
                        entered.send(()).unwrap();
                        release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                    },
                )
            }
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
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
        assert!(prepared.is_cancelled_for_test());
        assert_eq!(
            gate.execute_for_test(prepared, scheduler).await,
            Err(PluginNetworkErrorCode::ExpiredRequest)
        );
    }

    #[test]
    fn credential_missing_invalid_records_and_tokens_have_one_fixed_error() {
        let fixture = SecretFixture::new();
        let gate = PluginNetworkAuthorityGate::with_broker(Arc::new(
            PluginHttpsBroker::with_dependencies(
                PluginNetworkRequestRegistry::default(),
                resolver(),
                Arc::new(DeterministicHttpsTransport::new(vec![])),
            ),
        ));
        let (scheduler, context) = scheduler(1, 2, 3);
        gate.publish(Some(bearer_snapshot())).unwrap();
        let consume = || {
            gate.admit_with_secrets(
                "com.example.network",
                1,
                &context,
                &scheduler,
                bearer_request(),
                &fixture.store,
            )
            .err()
        };
        assert_eq!(
            consume(),
            Some(PluginNetworkErrorCode::CredentialUnavailable)
        );
        for token in ["", "Bearer wrong", "contains space", "valid-token"] {
            fixture
                .store
                .write("com.example.network", "api-token", token)
                .unwrap();
            if token == "valid-token" {
                let path = std::fs::read_dir(fixture.root.join("com.example.network"))
                    .unwrap()
                    .next()
                    .unwrap()
                    .unwrap()
                    .path();
                std::fs::write(path, b"corrupt synthetic record").unwrap();
            }
            assert_eq!(
                consume(),
                Some(PluginNetworkErrorCode::CredentialUnavailable)
            );
        }
        fixture
            .store
            .write("com.example.network", "api-token", "valid-token")
            .unwrap();
        let path = std::fs::read_dir(fixture.root.join("com.example.network"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let mut bytes = std::fs::read(&path).unwrap();
        *bytes.last_mut().unwrap() ^= 0xff;
        std::fs::write(path, bytes).unwrap();
        assert_eq!(
            consume(),
            Some(PluginNetworkErrorCode::CredentialUnavailable)
        );
        let wire = serde_json::to_value(
            crate::public_plugins::runtime::PluginNetworkCommandError::from(
                PluginNetworkErrorCode::CredentialUnavailable,
            ),
        )
        .unwrap();
        assert_eq!(wire, serde_json::json!({"code":"credentialUnavailable"}));
        gate.publish_plugin("com.example.network", None).unwrap();
        assert_eq!(consume(), Some(PluginNetworkErrorCode::PermissionDenied));
    }
}
