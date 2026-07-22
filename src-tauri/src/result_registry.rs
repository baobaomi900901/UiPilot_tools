use std::{
    collections::HashMap,
    fmt,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

use crate::apps::ApplicationLaunchTarget;
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ResultAction {
    LaunchApplication {
        app_id: String,
        target: ApplicationLaunchTarget,
    },
    OpenIndexedPath,
    CopyText {
        plugin_id: String,
        text: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueryDomain {
    Application,
    File,
    Plugin,
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
    domain: QueryDomain,
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
    latest_query_domain: Option<QueryDomain>,
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
    pub(crate) fn on_show(&self, invocation_id: String) {
        let mut state = self.state.lock().expect("result registry lock poisoned");
        state.generation = state
            .generation
            .checked_add(1)
            .expect("result registry generation exhausted");
        state.active = true;
        state.active_invocation_id = Some(invocation_id);
        state.latest_query_sequence = 0;
        state.latest_query_domain = None;
        state.current = None;
    }

    pub(crate) fn begin_query(
        &self,
        domain: QueryDomain,
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
        state.latest_query_domain = Some(domain);
        state.current = None;
        Some(QueryToken {
            generation: state.generation,
            query_sequence,
            domain,
        })
    }

    pub(crate) fn publish_if_latest<T, R, A, F>(
        &self,
        token: QueryToken,
        entries: Vec<(T, ResultAction)>,
        authorize: A,
        response: F,
    ) -> Option<R>
    where
        A: FnOnce() -> bool,
        F: FnOnce(String, Vec<(String, T)>) -> R,
    {
        let mut state = self.state.lock().expect("result registry lock poisoned");
        if !state.active
            || token.generation != state.generation
            || token.query_sequence != state.latest_query_sequence
            || Some(token.domain) != state.latest_query_domain
            || !authorize()
        {
            return None;
        }

        let request_id = self.allocate_id("req");
        let mut items = Vec::with_capacity(entries.len());
        let mut actions = HashMap::with_capacity(entries.len());
        for (item, action) in entries {
            let result_id = self.allocate_id("item");
            actions.insert(result_id.clone(), action);
            items.push((result_id, item));
        }

        state.current = Some(ResultSet {
            request_id: request_id.clone(),
            actions,
        });
        Some(response(request_id, items))
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
        state.latest_query_domain = None;
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
    use std::{cell::Cell, path::PathBuf};

    use serde_json::Value;

    use super::{QueryDomain, QueryToken, RegistryError, ResultAction, ResultRegistry};
    use crate::{
        apps::ApplicationLaunchTarget,
        model::{ResultItem, SearchResponse},
    };

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct FileDraft {
        name: &'static str,
    }

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
            target: ApplicationLaunchTarget::Shortcut {
                shortcut: PathBuf::from(format!(r"C:\private\{name}.lnk")),
                executable: Some(PathBuf::from(format!(r"C:\private\{name}.exe"))),
            },
        }
    }

    fn publish_app(
        registry: &ResultRegistry,
        token: QueryToken,
        entries: Vec<(ResultItem, ResultAction)>,
    ) -> Option<SearchResponse> {
        registry.publish_if_latest(
            token,
            entries,
            || true,
            |request_id, items| SearchResponse {
                request_id,
                items: items
                    .into_iter()
                    .map(|(result_id, mut item)| {
                        item.result_id = result_id;
                        item
                    })
                    .collect(),
            },
        )
    }

    #[test]
    fn query_domains_share_one_invocation_sequence_and_mapping() {
        let registry = ResultRegistry::default();
        registry.on_show("inv-1".into());

        let application = registry
            .begin_query(QueryDomain::Application, "inv-1", 1)
            .unwrap();
        let mut application_item = item("", "Application");
        application_item.icon = Some("data:image/png;base64,iVBORw==".into());
        let application_response = publish_app(
            &registry,
            application,
            vec![(application_item, action("application"))],
        )
        .unwrap();
        assert_eq!(
            application_response.items[0].icon.as_deref(),
            Some("data:image/png;base64,iVBORw==")
        );

        let file = registry.begin_query(QueryDomain::File, "inv-1", 2).unwrap();
        assert_eq!(
            registry.resolve(
                &application_response.request_id,
                &application_response.items[0].result_id,
            ),
            Err(RegistryError::StaleRequest)
        );
        assert!(registry
            .begin_query(QueryDomain::Application, "inv-1", 1)
            .is_none());

        let file_response = registry
            .publish_if_latest(
                file,
                vec![
                    (FileDraft { name: "First" }, ResultAction::OpenIndexedPath),
                    (FileDraft { name: "Second" }, ResultAction::OpenIndexedPath),
                ],
                || true,
                |request_id, items| (request_id, items),
            )
            .unwrap();
        assert_eq!(file_response.0, "req-0000000000000003");
        assert_eq!(file_response.1[0].0, "item-0000000000000004");
        assert_eq!(file_response.1[1].0, "item-0000000000000005");
        assert_eq!(
            registry.resolve(&file_response.0, &file_response.1[0].0),
            Ok(ResultAction::OpenIndexedPath)
        );

        registry.hide_and_clear();
        assert_eq!(
            registry.resolve(&file_response.0, &file_response.1[0].0),
            Err(RegistryError::StaleRequest)
        );
    }

    #[test]
    fn query_domains_accept_plugin_domain() {
        let registry = ResultRegistry::default();
        registry.on_show("inv-1".into());
        let plugin = registry
            .begin_query(QueryDomain::Plugin, "inv-1", 1)
            .unwrap();
        let response = registry
            .publish_if_latest(
                plugin,
                vec![(
                    item("", "Plugin"),
                    ResultAction::CopyText {
                        plugin_id: "plugin".into(),
                        text: "copy".into(),
                    },
                )],
                || true,
                |request_id, items| (request_id, items),
            )
            .unwrap();
        assert_eq!(response.0, "req-0000000000000001");
        assert_eq!(
            registry.resolve(&response.0, &response.1[0].0),
            Ok(ResultAction::CopyText {
                plugin_id: "plugin".into(),
                text: "copy".into(),
            })
        );
    }

    #[test]
    fn token_domain_tamper_is_fail_closed_without_consuming_ids() {
        let registry = ResultRegistry::default();
        registry.on_show("inv-1".into());
        let token = registry.begin_query(QueryDomain::File, "inv-1", 1).unwrap();
        let tampered = QueryToken {
            domain: QueryDomain::Application,
            ..token
        };

        assert!(registry
            .publish_if_latest(
                tampered,
                vec![(FileDraft { name: "Wrong" }, ResultAction::OpenIndexedPath)],
                || true,
                |request_id, items| (request_id, items),
            )
            .is_none());

        let current = registry
            .publish_if_latest(
                token,
                vec![(FileDraft { name: "Current" }, ResultAction::OpenIndexedPath)],
                || true,
                |request_id, items| (request_id, items),
            )
            .unwrap();
        assert_eq!(current.0, "req-0000000000000001");
        assert_eq!(current.1[0].0, "item-0000000000000002");
    }

    #[test]
    fn generic_publication_reuses_existing_ids_mapping_and_hide() {
        let registry = ResultRegistry::default();
        registry.on_show("inv-1".into());
        let token = registry
            .begin_query(QueryDomain::Application, "inv-1", 1)
            .unwrap();
        let expected = [action("first"), action("second")];
        let response = registry
            .publish_if_latest(
                token,
                vec![
                    (FileDraft { name: "First" }, expected[0].clone()),
                    (FileDraft { name: "Second" }, expected[1].clone()),
                ],
                || true,
                |request_id, items| (request_id, items),
            )
            .unwrap();

        assert_eq!(response.0, "req-0000000000000001");
        assert_eq!(response.1[0].0, "item-0000000000000002");
        assert_eq!(response.1[1].0, "item-0000000000000003");
        assert_eq!(
            registry.resolve(&response.0, &response.1[0].0),
            Ok(expected[0].clone())
        );
        assert_eq!(
            registry.resolve(&response.0, &response.1[1].0),
            Ok(expected[1].clone())
        );

        registry.hide_and_clear();
        assert_eq!(
            registry.resolve(&response.0, &response.1[0].0),
            Err(RegistryError::StaleRequest)
        );
        assert_eq!(
            registry.resolve(&response.0, &response.1[1].0),
            Err(RegistryError::StaleRequest)
        );
    }

    #[test]
    fn authorization_rejection_has_zero_side_effects_and_consumes_no_ids() {
        let registry = ResultRegistry::default();
        registry.on_show("inv-1".into());
        let token = registry
            .begin_query(QueryDomain::Application, "inv-1", 1)
            .unwrap();
        let current_action = action("current");
        let current = publish_app(
            &registry,
            token,
            vec![(item("", "Current"), current_action.clone())],
        )
        .unwrap();
        let response_called = Cell::new(false);

        assert!(registry
            .publish_if_latest(
                token,
                vec![(FileDraft { name: "Rejected" }, action("rejected"))],
                || false,
                |request_id, items| {
                    response_called.set(true);
                    (request_id, items)
                },
            )
            .is_none());
        assert!(!response_called.get());
        assert_eq!(
            registry
                .resolve(&current.request_id, &current.items[0].result_id)
                .unwrap(),
            current_action
        );

        let accepted = registry
            .publish_if_latest(
                token,
                vec![(FileDraft { name: "Accepted" }, action("accepted"))],
                || true,
                |request_id, items| (request_id, items),
            )
            .unwrap();
        assert_eq!(accepted.0, "req-0000000000000003");
        assert_eq!(accepted.1[0].0, "item-0000000000000004");
    }

    #[test]
    fn latest_publish_assigns_opaque_ids_and_replaces_supplied_ids() {
        let registry = ResultRegistry::default();
        registry.on_show("invocation-1".into());
        let token = registry
            .begin_query(QueryDomain::Application, "invocation-1", 1)
            .unwrap();

        let response = publish_app(
            &registry,
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
        let expected = ResultAction::LaunchApplication {
            app_id: "app-calculator".into(),
            target: ApplicationLaunchTarget::PackagedApp {
                aumid: "family!private-calculator".into(),
            },
        };
        registry.on_show("invocation-1".into());
        let token = registry
            .begin_query(QueryDomain::Application, "invocation-1", 1)
            .unwrap();
        let response = publish_app(
            &registry,
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
        assert!(!json.contains("family!private-calculator"));
        assert!(!json.contains("target"));
    }

    #[test]
    fn older_query_cannot_replace_newer_published_results() {
        let registry = ResultRegistry::default();
        registry.on_show("invocation-1".into());
        let first = registry
            .begin_query(QueryDomain::Application, "invocation-1", 1)
            .unwrap();
        let second = registry
            .begin_query(QueryDomain::Application, "invocation-1", 2)
            .unwrap();
        let expected = action("second");
        let current = publish_app(
            &registry,
            second,
            vec![(item("", "Second"), expected.clone())],
        )
        .unwrap();

        assert!(
            publish_app(&registry, first, vec![(item("", "First"), action("first"))],).is_none()
        );
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

        assert!(registry
            .begin_query(QueryDomain::Application, "invocation-1", 2)
            .is_some());
        assert!(registry
            .begin_query(QueryDomain::Application, "invocation-1", 1)
            .is_none());
    }

    #[test]
    fn valid_new_query_immediately_invalidates_published_results() {
        let registry = ResultRegistry::default();
        registry.on_show("invocation-1".into());
        let first = registry
            .begin_query(QueryDomain::Application, "invocation-1", 1)
            .unwrap();
        let response =
            publish_app(&registry, first, vec![(item("", "First"), action("first"))]).unwrap();

        assert!(registry
            .begin_query(QueryDomain::Application, "invocation-1", 2)
            .is_some());
        assert_eq!(
            registry.resolve(&response.request_id, &response.items[0].result_id),
            Err(RegistryError::StaleRequest)
        );
    }

    #[test]
    fn hidden_generation_rejects_in_flight_publish() {
        let registry = ResultRegistry::default();
        registry.on_show("invocation-1".into());
        let token = registry
            .begin_query(QueryDomain::Application, "invocation-1", 1)
            .unwrap();

        registry.hide_and_clear();

        assert!(publish_app(&registry, token, vec![(item("", "Late"), action("late"))],).is_none());
    }

    #[test]
    fn old_invocation_is_rejected_without_clearing_new_results() {
        let registry = ResultRegistry::default();
        registry.on_show("old-invocation".into());
        registry.on_show("new-invocation".into());
        let token = registry
            .begin_query(QueryDomain::Application, "new-invocation", 1)
            .unwrap();
        let expected = action("current");
        let current = publish_app(
            &registry,
            token,
            vec![(item("", "Current"), expected.clone())],
        )
        .unwrap();

        assert!(registry
            .begin_query(QueryDomain::Application, "old-invocation", 2)
            .is_none());
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
        let token = registry
            .begin_query(QueryDomain::Application, "invocation-1", 1)
            .unwrap();
        let response = publish_app(
            &registry,
            token,
            vec![(item("", "Secret"), action("secret"))],
        )
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
        let token = registry
            .begin_query(QueryDomain::Application, "invocation-1", 1)
            .unwrap();
        let response = publish_app(
            &registry,
            token,
            vec![(item("", "Calculator"), action("calculator"))],
        )
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
        let stale = registry
            .begin_query(QueryDomain::Application, "old-invocation", 1)
            .unwrap();

        registry.on_show("new-invocation".into());
        let current_token = registry
            .begin_query(QueryDomain::Application, "new-invocation", 1)
            .unwrap();
        let current_action = action("current");
        let current = publish_app(
            &registry,
            current_token,
            vec![(item("", "Current"), current_action.clone())],
        )
        .unwrap();

        assert!(
            publish_app(&registry, stale, vec![(item("", "Stale"), action("stale"))],).is_none()
        );
        assert_eq!(
            registry
                .resolve(&current.request_id, &current.items[0].result_id)
                .unwrap(),
            current_action
        );

        let next_token = registry
            .begin_query(QueryDomain::Application, "new-invocation", 2)
            .unwrap();
        let next = publish_app(
            &registry,
            next_token,
            vec![(item("", "Next"), action("next"))],
        )
        .unwrap();
        assert_eq!(next.request_id, "req-0000000000000003");
        assert_eq!(next.items[0].result_id, "item-0000000000000004");
    }
}
