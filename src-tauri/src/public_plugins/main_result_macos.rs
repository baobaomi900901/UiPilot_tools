//! Internal informational MainResult adapter using the real launcher registry.
//! No production IPC, clipboard grants, action execution or UI emission here.
use super::{
    runtime_data_macos::RuntimeDataBinding,
    runtime_result_macos::{MainCompletionError, PreparedMainResults},
};
use crate::{
    model::{search_response, LauncherResultActivation, ResultItem, SearchResponse},
    result_registry::{QueryDomain, QueryToken, ResultAction, ResultRegistry},
};
use std::sync::{Arc, Mutex};

pub(crate) struct MainResultPublisher {
    identity: Arc<()>,
    registry: ResultRegistry,
    #[cfg(test)]
    before_publish: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum CompletionState {
    Pending,
    Prepared,
    Terminal,
}
pub(crate) struct MainResultOwner {
    publisher: Arc<()>,
    identity: Arc<()>,
    query: QueryToken,
    binding: RuntimeDataBinding,
    state: Mutex<CompletionState>,
}
pub(crate) struct MainResultPublication {
    owner: Arc<()>,
    prepared: PreparedMainResults,
}
impl MainResultPublisher {
    pub(crate) fn new(registry: ResultRegistry) -> Self {
        Self {
            identity: Arc::new(()),
            registry,
            #[cfg(test)]
            before_publish: Mutex::new(None),
        }
    }
    /// Trusted query orchestration calls this with its captured invocation and
    /// sequence. Neither value nor the binding is reconstructed from plugin RPC.
    pub(crate) fn begin(
        &self,
        binding: RuntimeDataBinding,
        invocation: &str,
        sequence: u64,
    ) -> Option<MainResultOwner> {
        let query = self
            .registry
            .begin_query(QueryDomain::Plugin, invocation, sequence)?;
        Some(MainResultOwner {
            publisher: self.identity.clone(),
            identity: Arc::new(()),
            query,
            binding,
            state: Mutex::new(CompletionState::Pending),
        })
    }
    fn owns(&self, owner: &MainResultOwner) -> bool {
        Arc::ptr_eq(&self.identity, &owner.publisher)
    }

    pub(crate) fn prepare(
        &self,
        owner: &MainResultOwner,
        raw: &[u8],
    ) -> Result<MainResultPublication, MainCompletionError> {
        if !self.owns(owner) {
            return Err(MainCompletionError::Expired);
        }
        let mut state = owner
            .state
            .lock()
            .map_err(|_| MainCompletionError::Unavailable)?;
        if *state != CompletionState::Pending {
            return Err(MainCompletionError::Expired);
        }
        let prepared = match owner.binding.prepare_main_results(raw) {
            Ok(prepared) => prepared,
            Err(error) => {
                *state = CompletionState::Terminal;
                self.registry.retire_plugin_query_if_latest(owner.query);
                return Err(error);
            }
        };
        *state = CompletionState::Prepared;
        Ok(MainResultPublication {
            owner: owner.identity.clone(),
            prepared,
        })
    }

    /// Call only after transport terminal validation. Query ownership is checked
    /// by publish_if_latest under the same registry lock as the actual write.
    pub(crate) fn publish(
        &self,
        owner: &MainResultOwner,
        publication: MainResultPublication,
    ) -> Result<Option<SearchResponse>, MainCompletionError> {
        if !self.owns(owner) || !Arc::ptr_eq(&owner.identity, &publication.owner) {
            return Err(MainCompletionError::Expired);
        }
        let mut state = owner
            .state
            .lock()
            .map_err(|_| MainCompletionError::Unavailable)?;
        if *state != CompletionState::Prepared {
            return Err(MainCompletionError::Expired);
        }
        *state = CompletionState::Terminal;
        let result = owner
            .binding
            .submit_main_results(publication.prepared, |rows| {
                #[cfg(test)]
                if let Some(hook) = self.before_publish.lock().unwrap().take() {
                    hook();
                }
                let entries = rows
                    .into_iter()
                    .map(|row| {
                        (
                            ResultItem {
                                result_actions: None,
                                result_id: String::new(),
                                activation: LauncherResultActivation::Informational,
                                title: row.title,
                                subtitle: row.subtitle,
                                detail: row.detail,
                                icon: None,
                                plugin_icon_url: None,
                                icon_kind: None,
                                favorite: None,
                                has_default_action: false,
                            },
                            None::<ResultAction>,
                        )
                    })
                    .collect();
                // CallSession supplies request authority; these rows have no native
                // actions and require no fabricated clipboard/permission grant.
                self.registry
                    .publish_if_latest(owner.query, entries, || true, search_response)
            });
        if result.is_err() {
            self.registry.retire_plugin_query_if_latest(owner.query);
        }
        result
    }

    /// Completion failure is single-use. It cannot clear a success already
    /// published for this owner, even if an error arrives afterward.
    pub(crate) fn fail(&self, owner: &MainResultOwner) -> bool {
        if !self.owns(owner) {
            return false;
        }
        let Ok(mut state) = owner.state.lock() else {
            return false;
        };
        if *state == CompletionState::Terminal {
            return false;
        }
        *state = CompletionState::Terminal;
        self.registry.retire_plugin_query_if_latest(owner.query)
    }

    /// Host lifecycle retirement (hide/disable/request cancellation), distinct
    /// from a late completion error. RuntimeDataHost separately cancels/drains.
    pub(crate) fn retire(&self, owner: &MainResultOwner) -> bool {
        if !self.owns(owner) {
            return false;
        }
        let Ok(mut state) = owner.state.lock() else {
            return false;
        };
        *state = CompletionState::Terminal;
        self.registry.retire_plugin_query_if_latest(owner.query)
    }
}

#[cfg(test)]
#[path = "main_result_macos_tests.rs"]
mod tests;
