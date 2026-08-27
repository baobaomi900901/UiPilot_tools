use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use tokio_util::sync::CancellationToken;

use super::super::manifest::valid_plugin_id;

const MAX_CALLS_PER_CONTEXT: u8 = 8;
const MAX_CONCURRENT_PER_CONTEXT: usize = 2;
const MAX_CONCURRENT_HOST_WIDE: usize = 16;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct PluginNetworkContextIdentity {
    pub(super) plugin_id: String,
    pub(super) plugin_generation: u64,
    pub(super) activation_id: u64,
    pub(super) admission_epoch: u64,
    pub(super) request_id: String,
}

impl PluginNetworkContextIdentity {
    pub(super) fn new(
        plugin_id: &str,
        plugin_generation: u64,
        activation_id: u64,
        admission_epoch: u64,
        request_id: &str,
    ) -> Result<Self, PluginNetworkRegistryError> {
        if !valid_plugin_id(plugin_id)
            || plugin_generation == 0
            || activation_id == 0
            || admission_epoch == 0
            || request_id.is_empty()
            || request_id.len() > 128
        {
            return Err(PluginNetworkRegistryError::InvalidIdentity);
        }
        Ok(Self {
            plugin_id: plugin_id.to_owned(),
            plugin_generation,
            activation_id,
            admission_epoch,
            request_id: request_id.to_owned(),
        })
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct PluginNetworkCallIdentity {
    pub(super) context: PluginNetworkContextIdentity,
    pub(super) call_sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PluginNetworkCallTerminal {
    Delivered,
    Failed,
    Cancelled,
    Abandoned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PluginNetworkRegistryError {
    InvalidIdentity,
    LimitExceeded,
    Expired,
    Exhausted,
    Unavailable,
}

#[derive(Default)]
struct ContextUsage {
    attempts: u8,
    next_sequence: u64,
    active: usize,
    retired: bool,
}

struct InFlightCall {
    cancellation: CancellationToken,
}

#[derive(Default)]
struct RegistryState {
    contexts: HashMap<PluginNetworkContextIdentity, ContextUsage>,
    calls: HashMap<PluginNetworkCallIdentity, InFlightCall>,
    global_active: usize,
    terminal: bool,
    #[cfg(test)]
    terminals: HashMap<PluginNetworkCallIdentity, PluginNetworkCallTerminal>,
}

#[derive(Default)]
struct RegistryInner {
    state: Mutex<RegistryState>,
}

#[derive(Clone, Default)]
pub(super) struct PluginNetworkRequestRegistry {
    inner: Arc<RegistryInner>,
}

pub(super) struct PluginNetworkAttempt {
    identity: PluginNetworkCallIdentity,
}

impl PluginNetworkAttempt {
    pub(super) fn identity(&self) -> &PluginNetworkCallIdentity {
        &self.identity
    }
}

pub(super) struct RegisteredPluginNetworkCall {
    registry: PluginNetworkRequestRegistry,
    identity: PluginNetworkCallIdentity,
    cancellation: CancellationToken,
    finished: AtomicBool,
}

impl RegisteredPluginNetworkCall {
    pub(super) fn identity(&self) -> &PluginNetworkCallIdentity {
        &self.identity
    }

    pub(super) fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    pub(super) fn finish(&self, terminal: PluginNetworkCallTerminal) -> bool {
        if self.finished.swap(true, Ordering::AcqRel) {
            return false;
        }
        self.registry.finish(&self.identity, terminal)
    }
}

impl Drop for RegisteredPluginNetworkCall {
    fn drop(&mut self) {
        if !self.finished.swap(true, Ordering::AcqRel) {
            let _ = self
                .registry
                .finish(&self.identity, PluginNetworkCallTerminal::Abandoned);
        }
    }
}

impl PluginNetworkRequestRegistry {
    pub(super) fn reserve_attempt(
        &self,
        context: &PluginNetworkContextIdentity,
    ) -> Result<PluginNetworkAttempt, PluginNetworkRegistryError> {
        let mut state = self.lock()?;
        if state.terminal {
            return Err(PluginNetworkRegistryError::Unavailable);
        }
        let usage = state
            .contexts
            .entry(context.clone())
            .or_insert_with(|| ContextUsage {
                next_sequence: 1,
                ..ContextUsage::default()
            });
        if usage.retired {
            return Err(PluginNetworkRegistryError::Expired);
        }
        if usage.attempts >= MAX_CALLS_PER_CONTEXT {
            return Err(PluginNetworkRegistryError::LimitExceeded);
        }
        let call_sequence = usage.next_sequence;
        usage.next_sequence = call_sequence
            .checked_add(1)
            .ok_or(PluginNetworkRegistryError::Exhausted)?;
        usage.attempts = usage
            .attempts
            .checked_add(1)
            .ok_or(PluginNetworkRegistryError::Exhausted)?;
        Ok(PluginNetworkAttempt {
            identity: PluginNetworkCallIdentity {
                context: context.clone(),
                call_sequence,
            },
        })
    }

    pub(super) fn register(
        &self,
        attempt: PluginNetworkAttempt,
    ) -> Result<RegisteredPluginNetworkCall, PluginNetworkRegistryError> {
        let mut state = self.lock()?;
        if state.terminal {
            return Err(PluginNetworkRegistryError::Unavailable);
        }
        let context_active = state
            .contexts
            .get(&attempt.identity.context)
            .ok_or(PluginNetworkRegistryError::Expired)?;
        if context_active.retired {
            return Err(PluginNetworkRegistryError::Expired);
        }
        if context_active.active >= MAX_CONCURRENT_PER_CONTEXT
            || state.global_active >= MAX_CONCURRENT_HOST_WIDE
        {
            return Err(PluginNetworkRegistryError::LimitExceeded);
        }
        let next_context_active = context_active
            .active
            .checked_add(1)
            .ok_or(PluginNetworkRegistryError::Exhausted)?;
        let next_global_active = state
            .global_active
            .checked_add(1)
            .ok_or(PluginNetworkRegistryError::Exhausted)?;
        let cancellation = CancellationToken::new();
        match state.calls.entry(attempt.identity.clone()) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(InFlightCall {
                    cancellation: cancellation.clone(),
                });
            }
            std::collections::hash_map::Entry::Occupied(_) => {
                return Err(PluginNetworkRegistryError::Unavailable);
            }
        }
        let usage = state
            .contexts
            .get_mut(&attempt.identity.context)
            .ok_or(PluginNetworkRegistryError::Unavailable)?;
        usage.active = next_context_active;
        state.global_active = next_global_active;
        Ok(RegisteredPluginNetworkCall {
            registry: self.clone(),
            identity: attempt.identity,
            cancellation,
            finished: AtomicBool::new(false),
        })
    }

    pub(super) fn cancel_context(&self, context: &PluginNetworkContextIdentity) -> usize {
        self.cancel_matching(|candidate| candidate == context, false)
    }

    pub(super) fn cancel_request_context(
        &self,
        plugin_id: &str,
        plugin_generation: u64,
        request_id: &str,
    ) -> usize {
        self.cancel_matching(
            |context| {
                context.plugin_id == plugin_id
                    && context.plugin_generation == plugin_generation
                    && context.request_id == request_id
            },
            false,
        )
    }

    pub(super) fn cancel_runtime(
        &self,
        plugin_id: &str,
        plugin_generation: u64,
        activation_id: u64,
        admission_epoch: u64,
    ) -> usize {
        self.cancel_matching(
            |context| {
                context.plugin_id == plugin_id
                    && context.plugin_generation == plugin_generation
                    && context.activation_id == activation_id
                    && context.admission_epoch == admission_epoch
            },
            false,
        )
    }

    pub(super) fn cancel_generation(&self, plugin_id: &str, plugin_generation: u64) -> usize {
        self.cancel_matching(
            |context| {
                context.plugin_id == plugin_id && context.plugin_generation == plugin_generation
            },
            false,
        )
    }

    pub(super) fn cancel_plugin_except(
        &self,
        plugin_id: &str,
        retained: Option<(u64, u64, u64)>,
    ) -> usize {
        self.cancel_matching(
            |context| {
                context.plugin_id == plugin_id
                    && retained.is_none_or(|(generation, activation_id, admission_epoch)| {
                        context.plugin_generation != generation
                            || context.activation_id != activation_id
                            || context.admission_epoch != admission_epoch
                    })
            },
            false,
        )
    }

    pub(super) fn shutdown(&self) -> usize {
        self.cancel_matching(|_| true, true)
    }

    fn cancel_matching(
        &self,
        mut predicate: impl FnMut(&PluginNetworkContextIdentity) -> bool,
        terminal: bool,
    ) -> usize {
        let Ok(mut state) = self.lock() else {
            return 0;
        };
        if terminal {
            state.terminal = true;
        }
        for (context, usage) in &mut state.contexts {
            if predicate(context) {
                usage.retired = true;
            }
        }
        let identities = state
            .calls
            .keys()
            .filter(|identity| predicate(&identity.context))
            .cloned()
            .collect::<Vec<_>>();
        for identity in &identities {
            if let Some(call) = state.calls.remove(identity) {
                call.cancellation.cancel();
                let _ = release_slot(&mut state, &identity.context);
                #[cfg(test)]
                state
                    .terminals
                    .insert(identity.clone(), PluginNetworkCallTerminal::Cancelled);
            }
        }
        state
            .contexts
            .retain(|context, usage| !(usage.active == 0 && predicate(context)));
        identities.len()
    }

    fn finish(
        &self,
        identity: &PluginNetworkCallIdentity,
        _terminal: PluginNetworkCallTerminal,
    ) -> bool {
        let Ok(mut state) = self.lock() else {
            return false;
        };
        if state.calls.remove(identity).is_none() {
            return false;
        }
        if !release_slot(&mut state, &identity.context) {
            return false;
        }
        #[cfg(test)]
        state.terminals.insert(identity.clone(), _terminal);
        true
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, RegistryState>, PluginNetworkRegistryError> {
        self.inner
            .state
            .lock()
            .map_err(|_| PluginNetworkRegistryError::Unavailable)
    }

    #[cfg(test)]
    fn seed_next_sequence_for_test(
        &self,
        context: &PluginNetworkContextIdentity,
        next_sequence: u64,
    ) {
        self.lock()
            .unwrap()
            .contexts
            .entry(context.clone())
            .or_default()
            .next_sequence = next_sequence;
    }

    #[cfg(test)]
    fn active_counts_for_test(&self, context: &PluginNetworkContextIdentity) -> (usize, usize) {
        let state = self.lock().unwrap();
        (
            state.contexts.get(context).map_or(0, |usage| usage.active),
            state.global_active,
        )
    }

    #[cfg(test)]
    pub(super) fn global_active_for_test(&self) -> usize {
        self.lock().unwrap().global_active
    }

    #[cfg(test)]
    fn terminal_for_test(
        &self,
        identity: &PluginNetworkCallIdentity,
    ) -> Option<PluginNetworkCallTerminal> {
        self.lock().unwrap().terminals.get(identity).copied()
    }
}

fn release_slot(state: &mut RegistryState, context: &PluginNetworkContextIdentity) -> bool {
    let Some(next_global) = state.global_active.checked_sub(1) else {
        state.terminal = true;
        return false;
    };
    let Some(usage) = state.contexts.get_mut(context) else {
        state.terminal = true;
        return false;
    };
    let Some(next_context) = usage.active.checked_sub(1) else {
        state.terminal = true;
        return false;
    };
    usage.active = next_context;
    state.global_active = next_global;
    true
}

#[cfg(test)]
mod tests {
    use super::{
        PluginNetworkCallTerminal, PluginNetworkContextIdentity, PluginNetworkRegistryError,
        PluginNetworkRequestRegistry,
    };

    fn context(plugin: &str, request: &str) -> PluginNetworkContextIdentity {
        PluginNetworkContextIdentity::new(plugin, 1, 2, 3, request).unwrap()
    }

    #[test]
    fn plugin_network_registry_call_limit_and_sequence_never_wrap() {
        let registry = PluginNetworkRequestRegistry::default();
        let identity = context("com.example.network", "request-1");
        for sequence in 1..=8 {
            let attempt = registry.reserve_attempt(&identity).unwrap();
            assert_eq!(attempt.identity().call_sequence, sequence);
        }
        assert_eq!(
            registry.reserve_attempt(&identity).err(),
            Some(PluginNetworkRegistryError::LimitExceeded)
        );

        let exhausted = context("com.example.network", "request-exhausted");
        registry.seed_next_sequence_for_test(&exhausted, u64::MAX);
        assert_eq!(
            registry.reserve_attempt(&exhausted).err(),
            Some(PluginNetworkRegistryError::Exhausted)
        );
    }

    #[test]
    fn plugin_network_registry_enforces_context_and_host_concurrency_without_leaks() {
        let registry = PluginNetworkRequestRegistry::default();
        let first_context = context("com.example.first", "request-1");
        let first = registry
            .register(registry.reserve_attempt(&first_context).unwrap())
            .unwrap();
        let second = registry
            .register(registry.reserve_attempt(&first_context).unwrap())
            .unwrap();
        assert_eq!(
            registry
                .register(registry.reserve_attempt(&first_context).unwrap())
                .err(),
            Some(PluginNetworkRegistryError::LimitExceeded)
        );

        let mut host_calls = vec![first, second];
        for index in 0..14 {
            let identity = context(
                &format!("com.example.host-{index}"),
                &format!("request-{index}"),
            );
            host_calls.push(
                registry
                    .register(registry.reserve_attempt(&identity).unwrap())
                    .unwrap(),
            );
        }
        let overflow = context("com.example.overflow", "request-overflow");
        assert_eq!(
            registry
                .register(registry.reserve_attempt(&overflow).unwrap())
                .err(),
            Some(PluginNetworkRegistryError::LimitExceeded)
        );
        assert_eq!(registry.active_counts_for_test(&first_context), (2, 16));

        assert!(host_calls[0].finish(PluginNetworkCallTerminal::Failed));
        let replacement = registry
            .register(registry.reserve_attempt(&overflow).unwrap())
            .unwrap();
        assert_eq!(registry.active_counts_for_test(&first_context), (1, 16));
        drop(replacement);
        drop(host_calls);
        assert_eq!(registry.active_counts_for_test(&first_context), (0, 0));
    }

    #[test]
    fn plugin_network_registry_response_and_cancel_each_win_terminal_cas_once() {
        let registry = PluginNetworkRequestRegistry::default();
        let response_context = context("com.example.response", "request-response");
        let response = registry
            .register(registry.reserve_attempt(&response_context).unwrap())
            .unwrap();
        let response_identity = response.identity().clone();
        assert!(response.finish(PluginNetworkCallTerminal::Delivered));
        assert_eq!(registry.cancel_context(&response_context), 0);
        assert_eq!(
            registry.terminal_for_test(&response_identity),
            Some(PluginNetworkCallTerminal::Delivered)
        );

        let cancel_context = context("com.example.cancel", "request-cancel");
        let cancelled = registry
            .register(registry.reserve_attempt(&cancel_context).unwrap())
            .unwrap();
        let cancelled_identity = cancelled.identity().clone();
        assert_eq!(registry.cancel_context(&cancel_context), 1);
        assert!(cancelled.cancellation().is_cancelled());
        assert!(!cancelled.finish(PluginNetworkCallTerminal::Delivered));
        assert_eq!(
            registry.terminal_for_test(&cancelled_identity),
            Some(PluginNetworkCallTerminal::Cancelled)
        );
        assert_eq!(registry.active_counts_for_test(&cancel_context), (0, 0));
    }

    #[test]
    fn plugin_network_registry_stale_identity_cannot_cancel_newer_context() {
        let registry = PluginNetworkRequestRegistry::default();
        let stale = context("com.example.network", "request-shared");
        let current =
            PluginNetworkContextIdentity::new("com.example.network", 2, 4, 5, "request-shared")
                .unwrap();
        let current_call = registry
            .register(registry.reserve_attempt(&current).unwrap())
            .unwrap();
        assert_eq!(registry.cancel_context(&stale), 0);
        assert!(!current_call.cancellation().is_cancelled());
        assert!(current_call.finish(PluginNetworkCallTerminal::Delivered));
    }
}
