use serde::Serialize;

use crate::{IndexedScope, OperationCounters, SearchStatus, SpikeError, validate_literal};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub display_name: String,
    pub parsing_path: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryEvidence {
    pub literal: String,
    pub included_file_roots: Vec<String>,
    pub items: Vec<SearchHit>,
    pub counters: OperationCounters,
}

pub trait SearchBackend {
    fn status(&self) -> Result<SearchStatus, SpikeError>;
    fn indexed_scopes(&self) -> Result<Vec<IndexedScope>, SpikeError>;
    fn query_literal(
        &self,
        literal: &str,
        limit: u32,
        scopes: &[IndexedScope],
    ) -> Result<Vec<SearchHit>, SpikeError>;
}

pub trait QueryOperations {
    fn create_condition_leaf(&mut self, literal: &str) -> Result<(), SpikeError>;
    fn create_search_folder_factory(&mut self) -> Result<(), SpikeError>;
    fn set_condition(&mut self) -> Result<(), SpikeError>;
    fn set_display_name(&mut self) -> Result<(), SpikeError>;
    fn set_explicit_scopes(&mut self, scopes: &[IndexedScope]) -> Result<(), SpikeError>;
    fn get_shell_item(&mut self) -> Result<(), SpikeError>;
    fn enumerate(&mut self, limit: u32) -> Result<Vec<SearchHit>, SpikeError>;
}

pub fn execute_indexed_literal_query<B: SearchBackend>(
    backend: &B,
    literal: &str,
    limit: u32,
) -> Result<QueryEvidence, SpikeError> {
    validate_literal(literal)?;
    if !(1..=100).contains(&limit) {
        return Err(SpikeError::invalid_input(
            "limit must be an integer from 1 to 100",
        ));
    }

    let status = backend.status()?;
    if !status.service_running || !status.catalog_available || status.catalog != "SystemIndex" {
        return Err(SpikeError::not_runnable(
            "Windows Search service or SystemIndex is unavailable",
        ));
    }
    let scopes = backend.indexed_scopes()?;
    if scopes.is_empty() {
        return Err(SpikeError::not_runnable(
            "no provable indexed local file scope is available",
        ));
    }
    let items = backend.query_literal(literal, limit, &scopes)?;
    Ok(QueryEvidence {
        literal: literal.to_owned(),
        included_file_roots: scopes.iter().map(|scope| scope.url.clone()).collect(),
        items,
        counters: OperationCounters {
            search_folder_factory_created: 1,
            scope_set: 1,
            search_folder_enumerated: 1,
        },
    })
}

pub fn run_query_operations<O: QueryOperations>(
    operations: &mut O,
    literal: &str,
    limit: u32,
    scopes: &[IndexedScope],
) -> Result<Vec<SearchHit>, SpikeError> {
    let mut counters = OperationCounters::default();

    operations.create_condition_leaf(literal)?;
    operations.create_search_folder_factory()?;
    counters.search_folder_factory_created = 1;
    operations
        .set_condition()
        .map_err(|error| error.with_counters(counters))?;
    operations
        .set_display_name()
        .map_err(|error| error.with_counters(counters))?;
    operations
        .set_explicit_scopes(scopes)
        .map_err(|error| error.with_counters(counters))?;
    counters.scope_set = 1;
    operations
        .get_shell_item()
        .map_err(|error| error.with_counters(counters))?;
    counters.search_folder_enumerated = 1;
    let items = operations
        .enumerate(limit)
        .map_err(|error| error.with_counters(counters))?;

    Ok(items)
}
