use std::{
    collections::HashMap,
    fmt,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use crate::{apps::ApplicationLaunchTarget, file_search::FileExecutionAction};
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ResultAction {
    LaunchApplication {
        app_id: String,
        target: ApplicationLaunchTarget,
    },
    OpenFile(FileExecutionAction),
    CopyText {
        plugin_id: String,
        generation: u64,
        text: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowScope {
    Main,
    Find,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CounterExhausted;

#[derive(Clone, Copy, Debug)]
pub(crate) struct QueryToken {
    generation: u64,
    query_sequence: u64,
    domain: QueryDomain,
    domain_epoch: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DomainEpochExhausted;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PluginDomainEpochReservation {
    expected: u64,
    next: u64,
    nonce: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PluginDomainReservationError {
    Busy,
    Exhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PluginDomainReservationMismatch;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionTicket {
    scope: WindowScope,
    scope_generation: u64,
    invocation_id: String,
    result_set_generation: u64,
    request_id: String,
    result_id: String,
}

impl ExecutionTicket {
    pub(crate) const fn scope(&self) -> WindowScope {
        self.scope
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreparedApplicationQueryRetirement {
    scope_generation: u64,
    invocation_id: String,
    query_sequence: u64,
    expected_domain_epoch: u64,
    next_domain_epoch: u64,
}

struct ResultSet {
    request_id: String,
    domain: QueryDomain,
    generation: u64,
    actions: HashMap<String, ResultAction>,
}

#[derive(Default)]
struct RegistryState {
    generation: u64,
    result_set_generation: u64,
    active: bool,
    active_invocation_id: Option<String>,
    latest_query_sequence: u64,
    latest_query_domain: Option<QueryDomain>,
    domain_epochs: [u64; 3],
    domain_exhausted: [bool; 3],
    plugin_reservation: Option<PluginDomainEpochReservation>,
    current: Option<ResultSet>,
}

#[derive(Default)]
struct OpaqueIdAllocator {
    next_id: AtomicU64,
}

impl OpaqueIdAllocator {
    fn allocate(&self, count: u64) -> Option<u64> {
        self.next_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(count)
            })
            .ok()
    }
}

struct RegistryInner {
    allocator: Arc<OpaqueIdAllocator>,
    next_reservation_nonce: AtomicU64,
    state: Mutex<RegistryState>,
    scope: WindowScope,
    restrict_domains: bool,
}

#[derive(Clone)]
pub(crate) struct ResultRegistry {
    inner: Arc<RegistryInner>,
}

#[derive(Clone)]
pub(crate) struct ResultRegistries {
    main: ResultRegistry,
    find: ResultRegistry,
}

impl Default for ResultRegistries {
    fn default() -> Self {
        let allocator = Arc::new(OpaqueIdAllocator::default());
        Self {
            main: ResultRegistry::scoped(WindowScope::Main, Arc::clone(&allocator)),
            find: ResultRegistry::scoped(WindowScope::Find, Arc::clone(&allocator)),
        }
    }
}

impl ResultRegistries {
    pub(crate) fn main(&self) -> &ResultRegistry {
        &self.main
    }

    pub(crate) fn find(&self) -> &ResultRegistry {
        &self.find
    }
}

impl Default for ResultRegistry {
    fn default() -> Self {
        // Kept unrestricted until production managed state migrates to ResultRegistries.
        Self::new(
            WindowScope::Main,
            Arc::new(OpaqueIdAllocator::default()),
            false,
        )
    }
}

impl ResultRegistry {
    fn scoped(scope: WindowScope, allocator: Arc<OpaqueIdAllocator>) -> Self {
        Self::new(scope, allocator, true)
    }

    fn new(scope: WindowScope, allocator: Arc<OpaqueIdAllocator>, restrict_domains: bool) -> Self {
        Self {
            inner: Arc::new(RegistryInner {
                allocator,
                next_reservation_nonce: AtomicU64::new(0),
                state: Mutex::new(RegistryState::default()),
                scope,
                restrict_domains,
            }),
        }
    }

    pub(crate) fn on_show(&self, invocation_id: String) {
        let _ = self.try_on_show(invocation_id);
    }

    pub(crate) fn try_on_show(&self, invocation_id: String) -> Result<(), CounterExhausted> {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("result registry lock poisoned");
        let Some(next_generation) = state.generation.checked_add(1) else {
            Self::clear_state(&mut state);
            return Err(CounterExhausted);
        };
        state.generation = next_generation;
        state.active = true;
        state.active_invocation_id = Some(invocation_id);
        state.latest_query_sequence = 0;
        state.latest_query_domain = None;
        state.current = None;
        Ok(())
    }

    pub(crate) fn begin_query(
        &self,
        domain: QueryDomain,
        invocation_id: &str,
        query_sequence: u64,
    ) -> Option<QueryToken> {
        if !self.allows_domain(domain) {
            return None;
        }
        let mut state = self
            .inner
            .state
            .lock()
            .expect("result registry lock poisoned");
        if !state.active
            || state.active_invocation_id.as_deref() != Some(invocation_id)
            || query_sequence <= state.latest_query_sequence
            || state.domain_exhausted[domain.index()]
            || (domain == QueryDomain::Plugin && state.plugin_reservation.is_some())
        {
            return None;
        }

        state.latest_query_sequence = query_sequence;
        state.latest_query_domain = Some(domain);
        state.current = None;
        let domain_epoch = state.domain_epochs[domain.index()];
        Some(QueryToken {
            generation: state.generation,
            query_sequence,
            domain,
            domain_epoch,
        })
    }

    pub(crate) fn publish_if_latest<T, R, E, A, F>(
        &self,
        token: QueryToken,
        entries: Vec<(T, E)>,
        authorize: A,
        response: F,
    ) -> Option<R>
    where
        E: Into<Option<ResultAction>>,
        A: FnOnce() -> bool,
        F: FnOnce(String, Vec<(String, T)>) -> R,
    {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("result registry lock poisoned");
        if !state.active
            || token.generation != state.generation
            || token.query_sequence != state.latest_query_sequence
            || Some(token.domain) != state.latest_query_domain
            || state.domain_exhausted[token.domain.index()]
            || (token.domain == QueryDomain::Plugin && state.plugin_reservation.is_some())
            || token.domain_epoch != state.domain_epochs[token.domain.index()]
            || !authorize()
        {
            return None;
        }

        let next_result_set_generation = match state.result_set_generation.checked_add(1) {
            Some(generation) => generation,
            None => {
                state.current = None;
                return None;
            }
        };
        let Some(id_count) = u64::try_from(entries.len())
            .ok()
            .and_then(|count| count.checked_add(1))
        else {
            state.current = None;
            return None;
        };
        let first_id = match self.inner.allocator.allocate(id_count) {
            Some(previous) => match previous.checked_add(1) {
                Some(first) => first,
                None => {
                    state.current = None;
                    return None;
                }
            },
            None => {
                state.current = None;
                return None;
            }
        };
        let request_id = Self::format_id("req", first_id);
        let mut items = Vec::with_capacity(entries.len());
        let mut actions = HashMap::with_capacity(entries.len());
        let mut allocated_id = first_id;
        for (item, action) in entries {
            allocated_id = allocated_id
                .checked_add(1)
                .expect("reserved opaque identifier range must be contiguous");
            let result_id = Self::format_id("item", allocated_id);
            if let Some(action) = action.into() {
                actions.insert(result_id.clone(), action);
            }
            items.push((result_id, item));
        }

        state.result_set_generation = next_result_set_generation;
        state.current = Some(ResultSet {
            request_id: request_id.clone(),
            domain: token.domain,
            generation: next_result_set_generation,
            actions,
        });
        Some(response(request_id, items))
    }

    pub(crate) fn resolve(
        &self,
        request_id: &str,
        result_id: &str,
    ) -> Result<ResultAction, RegistryError> {
        self.resolve_with_ticket(request_id, result_id)
            .map(|(action, _)| action)
    }

    pub(crate) fn resolve_with_ticket(
        &self,
        request_id: &str,
        result_id: &str,
    ) -> Result<(ResultAction, ExecutionTicket), RegistryError> {
        let state = self
            .inner
            .state
            .lock()
            .expect("result registry lock poisoned");
        let current = state.current.as_ref().ok_or(RegistryError::StaleRequest)?;
        if current.request_id != request_id {
            return Err(RegistryError::StaleRequest);
        }

        let action = current
            .actions
            .get(result_id)
            .cloned()
            .ok_or(RegistryError::UnknownResult)?;
        let invocation_id = state
            .active_invocation_id
            .as_ref()
            .ok_or(RegistryError::StaleRequest)?
            .clone();
        let ticket = ExecutionTicket {
            scope: self.inner.scope,
            scope_generation: state.generation,
            invocation_id,
            result_set_generation: current.generation,
            request_id: request_id.to_owned(),
            result_id: result_id.to_owned(),
        };
        Ok((action, ticket))
    }

    pub(crate) fn is_execution_ticket_current(&self, ticket: &ExecutionTicket) -> bool {
        let state = self
            .inner
            .state
            .lock()
            .expect("result registry lock poisoned");
        self.ticket_matches(&state, ticket)
    }

    pub(crate) fn retire_result_set_if_current(&self, ticket: &ExecutionTicket) -> bool {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("result registry lock poisoned");
        if !self.ticket_matches(&state, ticket) {
            return false;
        }
        state.current = None;
        true
    }

    pub(crate) fn hide_and_clear(&self) {
        let _ = self.try_hide_and_clear();
    }

    pub(crate) fn try_hide_and_clear(&self) -> Result<(), CounterExhausted> {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("result registry lock poisoned");
        let next_generation = state.generation.checked_add(1);
        Self::clear_state(&mut state);
        let Some(next_generation) = next_generation else {
            return Err(CounterExhausted);
        };
        state.generation = next_generation;
        Ok(())
    }

    pub(crate) fn prepare_application_query_retirement(
        &self,
        invocation_id: &str,
        query_sequence: u64,
    ) -> Result<Option<PreparedApplicationQueryRetirement>, CounterExhausted> {
        if !self.allows_domain(QueryDomain::Application) {
            return Ok(None);
        }
        let state = self
            .inner
            .state
            .lock()
            .expect("result registry lock poisoned");
        if !state.active
            || state.active_invocation_id.as_deref() != Some(invocation_id)
            || state.latest_query_sequence != query_sequence
            || state.latest_query_domain != Some(QueryDomain::Application)
            || state.domain_exhausted[QueryDomain::Application.index()]
        {
            return Ok(None);
        }
        let expected_domain_epoch = state.domain_epochs[QueryDomain::Application.index()];
        let next_domain_epoch = expected_domain_epoch
            .checked_add(1)
            .ok_or(CounterExhausted)?;
        Ok(Some(PreparedApplicationQueryRetirement {
            scope_generation: state.generation,
            invocation_id: invocation_id.to_owned(),
            query_sequence,
            expected_domain_epoch,
            next_domain_epoch,
        }))
    }

    pub(crate) fn retire_application_query_if_current(
        &self,
        prepared: PreparedApplicationQueryRetirement,
    ) -> bool {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("result registry lock poisoned");
        let index = QueryDomain::Application.index();
        if !self.allows_domain(QueryDomain::Application)
            || !state.active
            || state.generation != prepared.scope_generation
            || state.active_invocation_id.as_deref() != Some(&prepared.invocation_id)
            || state.latest_query_sequence != prepared.query_sequence
            || state.latest_query_domain != Some(QueryDomain::Application)
            || state.domain_exhausted[index]
            || state.domain_epochs[index] != prepared.expected_domain_epoch
            || prepared.expected_domain_epoch.checked_add(1) != Some(prepared.next_domain_epoch)
        {
            return false;
        }
        state.domain_epochs[index] = prepared.next_domain_epoch;
        if state
            .current
            .as_ref()
            .is_some_and(|current| current.domain == QueryDomain::Application)
        {
            state.current = None;
        }
        true
    }

    pub(crate) fn reserve_plugin_epoch(
        &self,
    ) -> Result<PluginDomainEpochReservation, PluginDomainReservationError> {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("result registry lock poisoned");
        let index = QueryDomain::Plugin.index();
        if state.domain_exhausted[index] {
            return Err(PluginDomainReservationError::Exhausted);
        }
        if state.plugin_reservation.is_some() {
            return Err(PluginDomainReservationError::Busy);
        }
        let expected = state.domain_epochs[index];
        let Some(next) = expected.checked_add(1) else {
            state.domain_exhausted[index] = true;
            if state
                .current
                .as_ref()
                .is_some_and(|current| current.domain == QueryDomain::Plugin)
            {
                state.current = None;
            }
            return Err(PluginDomainReservationError::Exhausted);
        };
        let nonce = self
            .inner
            .next_reservation_nonce
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| {
                state.domain_exhausted[index] = true;
                state.current = None;
                PluginDomainReservationError::Exhausted
            })?
            + 1;
        let reservation = PluginDomainEpochReservation {
            expected,
            next,
            nonce,
        };
        state.plugin_reservation = Some(reservation);
        Ok(reservation)
    }

    pub(crate) fn cancel_reserved_plugin_epoch(
        &self,
        reservation: PluginDomainEpochReservation,
    ) -> Result<(), PluginDomainReservationMismatch> {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("result registry lock poisoned");
        if state.plugin_reservation != Some(reservation)
            || state.domain_epochs[QueryDomain::Plugin.index()] != reservation.expected
        {
            return Err(PluginDomainReservationMismatch);
        }
        state.plugin_reservation = None;
        Ok(())
    }

    pub(crate) fn commit_reserved_plugin_epoch(
        &self,
        reservation: PluginDomainEpochReservation,
    ) -> Result<(), PluginDomainReservationMismatch> {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("result registry lock poisoned");
        let index = QueryDomain::Plugin.index();
        if state.plugin_reservation != Some(reservation)
            || state.domain_exhausted[index]
            || state.domain_epochs[index] != reservation.expected
            || reservation.expected.checked_add(1) != Some(reservation.next)
        {
            return Err(PluginDomainReservationMismatch);
        }
        state.domain_epochs[index] = reservation.next;
        state.plugin_reservation = None;
        if state
            .current
            .as_ref()
            .is_some_and(|current| current.domain == QueryDomain::Plugin)
        {
            state.current = None;
        }
        Ok(())
    }

    pub(crate) fn invalidate_domain(
        &self,
        domain: QueryDomain,
    ) -> Result<(), DomainEpochExhausted> {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("result registry lock poisoned");
        if domain == QueryDomain::Plugin && state.plugin_reservation.is_some() {
            state.domain_exhausted[domain.index()] = true;
            state.plugin_reservation = None;
            state.current = None;
            return Err(DomainEpochExhausted);
        }
        let epoch = &mut state.domain_epochs[domain.index()];
        let Some(next) = epoch.checked_add(1) else {
            state.domain_exhausted[domain.index()] = true;
            if state
                .current
                .as_ref()
                .is_some_and(|current| current.domain == domain)
            {
                state.current = None;
            }
            return Err(DomainEpochExhausted);
        };
        *epoch = next;
        if state
            .current
            .as_ref()
            .is_some_and(|current| current.domain == domain)
        {
            state.current = None;
        }
        Ok(())
    }

    fn allows_domain(&self, domain: QueryDomain) -> bool {
        if !self.inner.restrict_domains {
            return true;
        }
        matches!(
            (self.inner.scope, domain),
            (
                WindowScope::Main,
                QueryDomain::Application | QueryDomain::Plugin
            ) | (WindowScope::Find, QueryDomain::File)
        )
    }

    fn ticket_matches(&self, state: &RegistryState, ticket: &ExecutionTicket) -> bool {
        state.active
            && ticket.scope == self.inner.scope
            && ticket.scope_generation == state.generation
            && state.active_invocation_id.as_deref() == Some(&ticket.invocation_id)
            && state.current.as_ref().is_some_and(|current| {
                current.generation == ticket.result_set_generation
                    && current.request_id == ticket.request_id
                    && current.actions.contains_key(&ticket.result_id)
            })
    }

    fn clear_state(state: &mut RegistryState) {
        state.active = false;
        state.active_invocation_id = None;
        state.latest_query_sequence = 0;
        state.latest_query_domain = None;
        state.current = None;
    }

    fn format_id(prefix: &str, id: u64) -> String {
        format!("{prefix}-{id:016x}")
    }
}

impl QueryDomain {
    const fn index(self) -> usize {
        match self {
            Self::Application => 0,
            Self::File => 1,
            Self::Plugin => 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, path::PathBuf, sync::atomic::Ordering};

    use serde_json::Value;

    use super::{
        QueryDomain, QueryToken, RegistryError, ResultAction, ResultRegistries, ResultRegistry,
        WindowScope,
    };
    use crate::{
        apps::ApplicationLaunchTarget,
        file_index::{IndexedKind, OpenIndexedPath, VolumeIdentity},
        file_search::{
            windows::path_auth::AuthenticatedPathIdentity, EverythingPathAction,
            FileExecutionAction, FilePathKind, FileResultItem, FileResultKind, FileSearchResponse,
        },
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
            detail: None,
            has_default_action: true,
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

    fn file_action() -> ResultAction {
        ResultAction::OpenFile(FileExecutionAction::Indexed(OpenIndexedPath::for_test(
            0,
            1,
            VolumeIdentity::for_test(r"\\?\Volume{REGISTRY}\", 1, "ntfs"),
            "file.txt",
            IndexedKind::File,
        )))
    }

    fn indexed_action_for_test() -> OpenIndexedPath {
        OpenIndexedPath::for_test(
            0,
            1,
            VolumeIdentity::for_test(r"\\?\Volume{REGISTRY}\", 1, "ntfs"),
            "file.txt",
            IndexedKind::File,
        )
    }

    fn authenticated_identity_for_test(
        relative_path: &str,
        file_id: [u8; 16],
    ) -> AuthenticatedPathIdentity {
        AuthenticatedPathIdentity {
            display_path: r"C:\Visible\report.pdf".into(),
            volume_guid_path: r"\\?\Volume{EVERYTHING-SECRET}\".into(),
            relative_path: relative_path.into(),
            volume_serial: 42,
            file_id,
            kind: FilePathKind::File,
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

    fn publish_one(
        registry: &ResultRegistry,
        domain: QueryDomain,
        invocation_id: &str,
        query_sequence: u64,
        expected: ResultAction,
    ) -> (String, String) {
        let token = registry
            .begin_query(domain, invocation_id, query_sequence)
            .unwrap();
        registry
            .publish_if_latest(
                token,
                vec![((), expected)],
                || true,
                |request_id, items| (request_id, items[0].0.clone()),
            )
            .unwrap()
    }

    #[test]
    fn window_scopes_publish_concurrently_and_hide_independently() {
        let registries = ResultRegistries::default();
        let main = registries.main();
        let find = registries.find();
        main.on_show("main-invocation".into());
        find.on_show("find-invocation".into());

        let main_ids = publish_one(
            main,
            QueryDomain::Application,
            "main-invocation",
            1,
            action("main"),
        );
        let find_ids = publish_one(find, QueryDomain::File, "find-invocation", 1, file_action());

        assert_eq!(
            main_ids,
            (
                "req-0000000000000001".into(),
                "item-0000000000000002".into()
            )
        );
        assert_eq!(
            find_ids,
            (
                "req-0000000000000003".into(),
                "item-0000000000000004".into()
            )
        );
        assert_eq!(main.resolve(&main_ids.0, &main_ids.1), Ok(action("main")));
        assert_eq!(find.resolve(&find_ids.0, &find_ids.1), Ok(file_action()));
        assert_eq!(
            main.resolve(&find_ids.0, &find_ids.1),
            Err(RegistryError::StaleRequest)
        );
        assert!(main
            .begin_query(QueryDomain::File, "main-invocation", 2)
            .is_none());
        assert!(find
            .begin_query(QueryDomain::Application, "find-invocation", 2)
            .is_none());

        find.hide_and_clear();
        assert_eq!(
            find.resolve(&find_ids.0, &find_ids.1),
            Err(RegistryError::StaleRequest)
        );
        assert_eq!(main.resolve(&main_ids.0, &main_ids.1), Ok(action("main")));
    }

    #[test]
    fn prepared_application_retirement_is_a_non_failing_current_query_cas() {
        let registries = ResultRegistries::default();
        let main = registries.main();
        let find = registries.find();
        main.on_show("main-invocation".into());
        find.on_show("find-invocation".into());
        let application_ids = publish_one(
            main,
            QueryDomain::Application,
            "main-invocation",
            1,
            action("application"),
        );
        let prepared = main
            .prepare_application_query_retirement("main-invocation", 1)
            .unwrap()
            .unwrap();

        let newer_application_ids = publish_one(
            main,
            QueryDomain::Application,
            "main-invocation",
            2,
            action("newer-application"),
        );
        let plugin_reservation = main.reserve_plugin_epoch().unwrap();
        let find_ids = publish_one(find, QueryDomain::File, "find-invocation", 1, file_action());

        assert!(!main.retire_application_query_if_current(prepared));
        assert_eq!(
            main.resolve(&newer_application_ids.0, &newer_application_ids.1),
            Ok(action("newer-application"))
        );
        assert_eq!(
            main.reserve_plugin_epoch(),
            Err(super::PluginDomainReservationError::Busy)
        );
        main.cancel_reserved_plugin_epoch(plugin_reservation)
            .unwrap();
        assert_eq!(find.resolve(&find_ids.0, &find_ids.1), Ok(file_action()));
        assert!(main
            .begin_query(QueryDomain::Application, "main-invocation", 3)
            .is_some());
        assert_eq!(
            main.resolve(&application_ids.0, &application_ids.1),
            Err(RegistryError::StaleRequest)
        );
    }

    #[test]
    fn committed_application_retirement_keeps_invocation_active_and_plugin_epoch_unchanged() {
        let registries = ResultRegistries::default();
        let main = registries.main();
        main.on_show("main-invocation".into());
        let application_ids = publish_one(
            main,
            QueryDomain::Application,
            "main-invocation",
            1,
            action("application"),
        );
        let prepared = main
            .prepare_application_query_retirement("main-invocation", 1)
            .unwrap()
            .unwrap();

        assert!(main.retire_application_query_if_current(prepared));
        assert_eq!(
            main.resolve(&application_ids.0, &application_ids.1),
            Err(RegistryError::StaleRequest)
        );
        assert!(main
            .begin_query(QueryDomain::Plugin, "main-invocation", 2)
            .is_some());
    }

    #[test]
    fn execution_ticket_is_stale_after_newer_result_set_generation() {
        let registries = ResultRegistries::default();
        let find = registries.find();
        find.on_show("find-invocation".into());
        let first_ids = publish_one(find, QueryDomain::File, "find-invocation", 1, file_action());
        let (_, ticket) = find
            .resolve_with_ticket(&first_ids.0, &first_ids.1)
            .unwrap();

        let second_ids = publish_one(find, QueryDomain::File, "find-invocation", 2, file_action());

        assert_eq!(ticket.scope(), WindowScope::Find);
        assert!(!find.is_execution_ticket_current(&ticket));
        assert!(!find.retire_result_set_if_current(&ticket));
        assert_eq!(
            find.resolve(&second_ids.0, &second_ids.1),
            Ok(file_action())
        );
    }

    #[test]
    fn checked_scope_result_set_and_global_id_exhaustion_fail_closed() {
        for case in ["scope", "result-set", "opaque-id"] {
            let registries = ResultRegistries::default();
            let find = registries.find();
            find.on_show("find-invocation".into());
            match case {
                "scope" => {
                    find.inner.state.lock().unwrap().generation = u64::MAX;
                    assert_eq!(
                        find.try_on_show("next".into()),
                        Err(super::CounterExhausted)
                    );
                    assert!(find.begin_query(QueryDomain::File, "next", 1).is_none());
                }
                "result-set" => {
                    find.inner.state.lock().unwrap().result_set_generation = u64::MAX;
                    let token = find
                        .begin_query(QueryDomain::File, "find-invocation", 1)
                        .unwrap();
                    assert!(find
                        .publish_if_latest(
                            token,
                            vec![((), file_action())],
                            || true,
                            |request_id, items| (request_id, items),
                        )
                        .is_none());
                }
                "opaque-id" => {
                    find.inner
                        .allocator
                        .next_id
                        .store(u64::MAX - 1, Ordering::Release);
                    let token = find
                        .begin_query(QueryDomain::File, "find-invocation", 1)
                        .unwrap();
                    assert!(find
                        .publish_if_latest(
                            token,
                            vec![((), file_action())],
                            || true,
                            |request_id, items| (request_id, items),
                        )
                        .is_none());
                    assert_eq!(
                        find.inner.allocator.next_id.load(Ordering::Acquire),
                        u64::MAX - 1
                    );
                }
                _ => unreachable!(),
            }
        }
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
                    (FileDraft { name: "First" }, file_action()),
                    (FileDraft { name: "Second" }, file_action()),
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
            Ok(file_action())
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
                        generation: 1,
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
                generation: 1,
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
                vec![(FileDraft { name: "Wrong" }, file_action())],
                || true,
                |request_id, items| (request_id, items),
            )
            .is_none());

        let current = registry
            .publish_if_latest(
                token,
                vec![(FileDraft { name: "Current" }, file_action())],
                || true,
                |request_id, items| (request_id, items),
            )
            .unwrap();
        assert_eq!(current.0, "req-0000000000000001");
        assert_eq!(current.1[0].0, "item-0000000000000002");
    }

    #[test]
    fn clones_share_allocator_mapping_and_narrow_domain_epoch() {
        let registry = ResultRegistry::default();
        let recovery = registry.clone();
        registry.on_show("inv-1".into());
        let stale_file = registry.begin_query(QueryDomain::File, "inv-1", 1).unwrap();
        let application = registry
            .begin_query(QueryDomain::Application, "inv-1", 2)
            .unwrap();
        let application_response = publish_app(
            &registry,
            application,
            vec![(item("", "Application"), action("application"))],
        )
        .unwrap();

        recovery.invalidate_domain(QueryDomain::File).unwrap();
        assert!(recovery
            .publish_if_latest(
                stale_file,
                vec![(FileDraft { name: "Stale" }, file_action())],
                || true,
                |request_id, items| (request_id, items),
            )
            .is_none());
        assert_eq!(
            recovery.resolve(
                &application_response.request_id,
                &application_response.items[0].result_id,
            ),
            Ok(action("application"))
        );

        let next = recovery.begin_query(QueryDomain::File, "inv-1", 3).unwrap();
        let response = registry
            .publish_if_latest(
                next,
                vec![(FileDraft { name: "Current" }, file_action())],
                || true,
                |request_id, items| (request_id, items),
            )
            .unwrap();
        assert_eq!(response.0, "req-0000000000000003");
        assert_eq!(response.1[0].0, "item-0000000000000004");
    }

    #[test]
    fn exhausted_file_epoch_is_permanent_and_allocates_nothing() {
        let registry = ResultRegistry::default();
        registry.on_show("inv-1".into());
        registry.inner.state.lock().unwrap().domain_epochs[QueryDomain::File.index()] = u64::MAX;
        let token = registry.begin_query(QueryDomain::File, "inv-1", 1).unwrap();

        assert_eq!(
            registry.invalidate_domain(QueryDomain::File),
            Err(super::DomainEpochExhausted)
        );
        assert!(registry
            .publish_if_latest(
                token,
                vec![(FileDraft { name: "Stale" }, file_action())],
                || true,
                |request_id, items| (request_id, items),
            )
            .is_none());
        assert!(registry
            .begin_query(QueryDomain::File, "inv-1", 2)
            .is_none());
        assert_eq!(registry.inner.allocator.next_id.load(Ordering::Acquire), 0);

        let application = registry
            .begin_query(QueryDomain::Application, "inv-1", 2)
            .unwrap();
        assert!(publish_app(
            &registry,
            application,
            vec![(item("", "Application"), action("application"))],
        )
        .is_some());
    }

    #[test]
    fn plugin_epoch_reservation_blocks_tokens_and_cancel_restores_current_epoch() {
        let registry = ResultRegistry::default();
        registry.on_show("inv-1".into());
        let old = registry
            .begin_query(QueryDomain::Plugin, "inv-1", 1)
            .unwrap();
        let reservation = registry.reserve_plugin_epoch().unwrap();

        assert!(registry
            .begin_query(QueryDomain::Plugin, "inv-1", 2)
            .is_none());
        assert!(registry
            .publish_if_latest(
                old,
                vec![(item("", "Plugin"), action("old"))],
                || true,
                |request_id, items| (request_id, items),
            )
            .is_none());
        registry.cancel_reserved_plugin_epoch(reservation).unwrap();
        assert!(registry
            .begin_query(QueryDomain::Plugin, "inv-1", 2)
            .is_some());
    }

    #[test]
    fn committed_plugin_epoch_rejects_old_token_and_clears_plugin_result() {
        let registry = ResultRegistry::default();
        registry.on_show("inv-1".into());
        let old = registry
            .begin_query(QueryDomain::Plugin, "inv-1", 1)
            .unwrap();
        let response = registry
            .publish_if_latest(
                old,
                vec![(item("", "Plugin"), action("old"))],
                || true,
                |request_id, items| (request_id, items),
            )
            .unwrap();
        let reservation = registry.reserve_plugin_epoch().unwrap();
        registry.commit_reserved_plugin_epoch(reservation).unwrap();

        assert_eq!(
            registry.resolve(&response.0, &response.1[0].0),
            Err(RegistryError::StaleRequest)
        );
        assert!(registry
            .publish_if_latest(
                old,
                vec![(item("", "Plugin"), action("old"))],
                || true,
                |request_id, items| (request_id, items),
            )
            .is_none());
    }

    #[test]
    fn plugin_epoch_allows_only_one_reservation() {
        let registry = ResultRegistry::default();
        let reservation = registry.reserve_plugin_epoch().unwrap();
        assert_eq!(
            registry.reserve_plugin_epoch(),
            Err(super::PluginDomainReservationError::Busy)
        );
        registry.cancel_reserved_plugin_epoch(reservation).unwrap();
    }

    #[test]
    fn plugin_epoch_overflow_is_terminal_without_a_reservation() {
        let registry = ResultRegistry::default();
        registry.on_show("inv-1".into());
        registry.inner.state.lock().unwrap().domain_epochs[QueryDomain::Plugin.index()] = u64::MAX;

        assert_eq!(
            registry.reserve_plugin_epoch(),
            Err(super::PluginDomainReservationError::Exhausted)
        );
        assert_eq!(
            registry.reserve_plugin_epoch(),
            Err(super::PluginDomainReservationError::Exhausted)
        );
        assert!(registry
            .begin_query(QueryDomain::Plugin, "inv-1", 1)
            .is_none());
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
    fn registry_resolves_indexed_and_everything_actions_as_opaque_file_actions() {
        let registry = ResultRegistry::default();
        registry.on_show("inv-1".into());
        let token = registry.begin_query(QueryDomain::File, "inv-1", 1).unwrap();
        let indexed = FileExecutionAction::Indexed(indexed_action_for_test());
        let everything = FileExecutionAction::Everything(EverythingPathAction::for_test(
            authenticated_identity_for_test(r"docs\report.pdf", [7; 16]),
        ));

        let response = registry
            .publish_if_latest(
                token,
                vec![
                    ("indexed", ResultAction::OpenFile(indexed.clone())),
                    ("everything", ResultAction::OpenFile(everything.clone())),
                ],
                || true,
                |request_id, items| (request_id, items),
            )
            .unwrap();

        assert_eq!(
            registry.resolve(&response.0, &response.1[0].0),
            Ok(ResultAction::OpenFile(indexed))
        );
        assert_eq!(
            registry.resolve(&response.0, &response.1[1].0),
            Ok(ResultAction::OpenFile(everything))
        );
    }

    #[test]
    fn file_search_response_serialization_omits_everything_identity() {
        let registry = ResultRegistry::default();
        registry.on_show("inv-1".into());
        let token = registry.begin_query(QueryDomain::File, "inv-1", 1).unwrap();
        let action = FileExecutionAction::Everything(EverythingPathAction::for_test(
            authenticated_identity_for_test(r"docs\report.pdf", [7; 16]),
        ));

        let response = registry
            .publish_if_latest(
                token,
                vec![("report", ResultAction::OpenFile(action))],
                || true,
                |request_id, items| FileSearchResponse {
                    request_id,
                    index_revision: "1".into(),
                    total: "1".into(),
                    status: crate::file_search::FileIndexStatus::Ready,
                    items: items
                        .into_iter()
                        .map(|(result_id, _)| FileResultItem {
                            result_id,
                            name: "report.pdf".into(),
                            kind: FileResultKind::File,
                            size_bytes: None,
                            modified_utc: "2026-07-30T00:00:00.000Z".into(),
                            full_path: r"C:\Visible\report.pdf".into(),
                        })
                        .collect(),
                },
            )
            .unwrap();

        let json = serde_json::to_value(response).unwrap().to_string();
        for private in [
            "identity",
            "volumeGuidPath",
            "fileId",
            r"\\?\Volume{EVERYTHING-SECRET}\",
            r"docs\report.pdf",
        ] {
            assert!(
                !json.contains(private),
                "serialized response exposes {private}"
            );
        }
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
