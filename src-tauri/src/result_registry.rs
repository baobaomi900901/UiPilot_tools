use std::{
    collections::HashMap,
    fmt,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

use crate::model::{ResultItem, SearchResponse};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ResultAction {
    LaunchApplication {
        app_id: String,
        shortcut: PathBuf,
        executable: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RegistryError {
    StaleRequest,
    UnknownResult,
}

impl fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::StaleRequest => "request is stale",
            Self::UnknownResult => "result is unknown",
        })
    }
}

impl std::error::Error for RegistryError {}

#[derive(Clone, Copy, Debug)]
pub(crate) struct QueryToken {
    generation: u64,
    query_sequence: u64,
}

struct ResultSet {
    request_id: String,
    actions: HashMap<String, ResultAction>,
}

#[derive(Default)]
struct RegistryState {
    generation: u64,
    active: bool,
    active_invocation_id: Option<String>,
    latest_query_sequence: u64,
    current: Option<ResultSet>,
}

pub(crate) struct ResultRegistry {
    next_id: AtomicU64,
    state: Mutex<RegistryState>,
}

impl Default for ResultRegistry {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(0),
            state: Mutex::new(RegistryState::default()),
        }
    }
}

impl ResultRegistry {
    #[cfg_attr(
        all(not(test), not(feature = "test-instrumentation")),
        allow(dead_code)
    )]
    pub(crate) fn on_show(&self, invocation_id: String) {
        let mut state = self.state.lock().expect("result registry lock poisoned");
        state.generation = state
            .generation
            .checked_add(1)
            .expect("result registry generation exhausted");
        state.active = true;
        state.active_invocation_id = Some(invocation_id);
        state.latest_query_sequence = 0;
        state.current = None;
    }

    pub(crate) fn begin_query(
        &self,
        invocation_id: &str,
        query_sequence: u64,
    ) -> Option<QueryToken> {
        let mut state = self.state.lock().expect("result registry lock poisoned");
        if !state.active
            || state.active_invocation_id.as_deref() != Some(invocation_id)
            || query_sequence <= state.latest_query_sequence
        {
            return None;
        }

        state.latest_query_sequence = query_sequence;
        state.current = None;
        Some(QueryToken {
            generation: state.generation,
            query_sequence,
        })
    }

    pub(crate) fn publish_if_latest(
        &self,
        token: QueryToken,
        entries: Vec<(ResultItem, ResultAction)>,
    ) -> Option<SearchResponse> {
        let mut state = self.state.lock().expect("result registry lock poisoned");
        if !state.active
            || token.generation != state.generation
            || token.query_sequence != state.latest_query_sequence
        {
            return None;
        }

        let request_id = self.allocate_id("req");
        let mut items = Vec::with_capacity(entries.len());
        let mut actions = HashMap::with_capacity(entries.len());
        for (mut item, action) in entries {
            item.result_id = self.allocate_id("item");
            actions.insert(item.result_id.clone(), action);
            items.push(item);
        }

        state.current = Some(ResultSet {
            request_id: request_id.clone(),
            actions,
        });
        Some(SearchResponse { request_id, items })
    }

    pub(crate) fn resolve(
        &self,
        request_id: &str,
        result_id: &str,
    ) -> Result<ResultAction, RegistryError> {
        let state = self.state.lock().expect("result registry lock poisoned");
        let current = state.current.as_ref().ok_or(RegistryError::StaleRequest)?;
        if current.request_id != request_id {
            return Err(RegistryError::StaleRequest);
        }

        current
            .actions
            .get(result_id)
            .cloned()
            .ok_or(RegistryError::UnknownResult)
    }

    pub(crate) fn hide_and_clear(&self) {
        let mut state = self.state.lock().expect("result registry lock poisoned");
        state.generation = state
            .generation
            .checked_add(1)
            .expect("result registry generation exhausted");
        state.active = false;
        state.active_invocation_id = None;
        state.latest_query_sequence = 0;
        state.current = None;
    }

    fn allocate_id(&self, prefix: &str) -> String {
        let previous = self
            .next_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .expect("result registry identifier space exhausted");
        format!("{prefix}-{:016x}", previous + 1)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::Value;

    use super::{RegistryError, ResultAction, ResultRegistry};
    use crate::model::ResultItem;

    fn item(result_id: &str, title: &str) -> ResultItem {
        ResultItem {
            result_id: result_id.to_owned(),
            title: title.to_owned(),
            subtitle: None,
            icon: None,
        }
    }

    fn action(name: &str) -> ResultAction {
        ResultAction::LaunchApplication {
            app_id: format!("app-{name}"),
            shortcut: PathBuf::from(format!(r"C:\private\{name}.lnk")),
            executable: Some(PathBuf::from(format!(r"C:\private\{name}.exe"))),
        }
    }

    #[test]
    fn latest_publish_assigns_opaque_ids_and_replaces_supplied_ids() {
        let registry = ResultRegistry::default();
        registry.on_show("invocation-1".into());
        let token = registry.begin_query("invocation-1", 1).unwrap();

        let response = registry
            .publish_if_latest(
                token,
                vec![
                    (item("forged-1", "First"), action("first")),
                    (item("forged-2", "Second"), action("second")),
                ],
            )
            .unwrap();

        assert_eq!(response.request_id, "req-0000000000000001");
        assert_eq!(response.items[0].result_id, "item-0000000000000002");
        assert_eq!(response.items[1].result_id, "item-0000000000000003");
        assert_ne!(response.items[0].result_id, "forged-1");
        assert_ne!(response.items[0].result_id, response.items[1].result_id);
    }

    #[test]
    fn current_ids_resolve_rust_owned_action_without_serializing_it() {
        let registry = ResultRegistry::default();
        let expected = action("calculator");
        registry.on_show("invocation-1".into());
        let token = registry.begin_query("invocation-1", 1).unwrap();
        let response = registry
            .publish_if_latest(
                token,
                vec![(item("forged", "Calculator"), expected.clone())],
            )
            .unwrap();

        assert_eq!(
            registry
                .resolve(&response.request_id, &response.items[0].result_id)
                .unwrap(),
            expected
        );
        let json = serde_json::to_string(&response).unwrap();
        assert!(!json.contains("private"));
        assert!(!json.contains("app-calculator"));
        assert!(!json.contains("shortcut"));
        assert!(!json.contains("executable"));
    }

    #[test]
    fn older_query_cannot_replace_newer_published_results() {
        let registry = ResultRegistry::default();
        registry.on_show("invocation-1".into());
        let first = registry.begin_query("invocation-1", 1).unwrap();
        let second = registry.begin_query("invocation-1", 2).unwrap();
        let expected = action("second");
        let current = registry
            .publish_if_latest(second, vec![(item("", "Second"), expected.clone())])
            .unwrap();

        assert!(registry
            .publish_if_latest(first, vec![(item("", "First"), action("first"))])
            .is_none());
        assert_eq!(
            registry
                .resolve(&current.request_id, &current.items[0].result_id)
                .unwrap(),
            expected
        );
    }

    #[test]
    fn lower_sequence_cannot_begin_after_higher_sequence() {
        let registry = ResultRegistry::default();
        registry.on_show("invocation-1".into());

        assert!(registry.begin_query("invocation-1", 2).is_some());
        assert!(registry.begin_query("invocation-1", 1).is_none());
    }

    #[test]
    fn valid_new_query_immediately_invalidates_published_results() {
        let registry = ResultRegistry::default();
        registry.on_show("invocation-1".into());
        let first = registry.begin_query("invocation-1", 1).unwrap();
        let response = registry
            .publish_if_latest(first, vec![(item("", "First"), action("first"))])
            .unwrap();

        assert!(registry.begin_query("invocation-1", 2).is_some());
        assert_eq!(
            registry.resolve(&response.request_id, &response.items[0].result_id),
            Err(RegistryError::StaleRequest)
        );
    }

    #[test]
    fn hidden_generation_rejects_in_flight_publish() {
        let registry = ResultRegistry::default();
        registry.on_show("invocation-1".into());
        let token = registry.begin_query("invocation-1", 1).unwrap();

        registry.hide_and_clear();

        assert!(registry
            .publish_if_latest(token, vec![(item("", "Late"), action("late"))])
            .is_none());
    }

    #[test]
    fn old_invocation_is_rejected_without_clearing_new_results() {
        let registry = ResultRegistry::default();
        registry.on_show("old-invocation".into());
        registry.on_show("new-invocation".into());
        let token = registry.begin_query("new-invocation", 1).unwrap();
        let expected = action("current");
        let current = registry
            .publish_if_latest(token, vec![(item("", "Current"), expected.clone())])
            .unwrap();

        assert!(registry.begin_query("old-invocation", 2).is_none());
        assert_eq!(
            registry
                .resolve(&current.request_id, &current.items[0].result_id)
                .unwrap(),
            expected
        );
    }

    #[test]
    fn unknown_and_stale_ids_return_fixed_path_free_errors() {
        let registry = ResultRegistry::default();
        registry.on_show("invocation-1".into());
        let token = registry.begin_query("invocation-1", 1).unwrap();
        let response = registry
            .publish_if_latest(token, vec![(item("", "Secret"), action("secret"))])
            .unwrap();

        let stale = registry
            .resolve("unknown-request", &response.items[0].result_id)
            .unwrap_err();
        let unknown = registry
            .resolve(&response.request_id, "unknown-result")
            .unwrap_err();

        assert_eq!(stale, RegistryError::StaleRequest);
        assert_eq!(unknown, RegistryError::UnknownResult);
        assert_eq!(stale.to_string(), "request is stale");
        assert_eq!(unknown.to_string(), "result is unknown");
        assert!(!stale.to_string().contains("private"));
        assert!(!unknown.to_string().contains("secret"));
    }

    #[test]
    fn response_serialization_is_camel_case_and_omits_unused_fields() {
        let registry = ResultRegistry::default();
        registry.on_show("invocation-1".into());
        let token = registry.begin_query("invocation-1", 1).unwrap();
        let response = registry
            .publish_if_latest(token, vec![(item("", "Calculator"), action("calculator"))])
            .unwrap();

        let json: Value = serde_json::to_value(response).unwrap();
        assert!(json.get("requestId").is_some());
        assert!(json.get("request_id").is_none());
        let result = &json["items"][0];
        assert!(result.get("resultId").is_some());
        assert!(result.get("result_id").is_none());
        assert!(result.get("kind").is_none());
        assert!(result.get("subtitle").is_none());
        assert!(result.get("icon").is_none());
        assert!(result.get("action").is_none());
    }

    #[test]
    fn stale_token_consumes_no_ids_and_cannot_partially_replace_current() {
        let registry = ResultRegistry::default();
        registry.on_show("old-invocation".into());
        let stale = registry.begin_query("old-invocation", 1).unwrap();

        registry.on_show("new-invocation".into());
        let current_token = registry.begin_query("new-invocation", 1).unwrap();
        let current_action = action("current");
        let current = registry
            .publish_if_latest(
                current_token,
                vec![(item("", "Current"), current_action.clone())],
            )
            .unwrap();

        assert!(registry
            .publish_if_latest(stale, vec![(item("", "Stale"), action("stale"))])
            .is_none());
        assert_eq!(
            registry
                .resolve(&current.request_id, &current.items[0].result_id)
                .unwrap(),
            current_action
        );

        let next_token = registry.begin_query("new-invocation", 2).unwrap();
        let next = registry
            .publish_if_latest(next_token, vec![(item("", "Next"), action("next"))])
            .unwrap();
        assert_eq!(next.request_id, "req-0000000000000003");
        assert_eq!(next.items[0].result_id, "item-0000000000000004");
    }
}
