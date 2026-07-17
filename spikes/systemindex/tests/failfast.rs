use systemindex_spike::{
    IndexedScope, SearchBackend, SearchHit, SearchStatus, SpikeError, execute_indexed_literal_query,
};

#[derive(Clone, Copy)]
enum Scenario {
    StoppedService,
    MissingCatalog,
    EmptyScopes,
    ScopeValidationFailure,
}

struct PanicOnQuery(Scenario);

impl SearchBackend for PanicOnQuery {
    fn status(&self) -> Result<SearchStatus, SpikeError> {
        let mut status = SearchStatus {
            catalog: "SystemIndex".to_owned(),
            service_running: true,
            catalog_available: true,
        };
        match self.0 {
            Scenario::StoppedService => status.service_running = false,
            Scenario::MissingCatalog => status.catalog_available = false,
            Scenario::EmptyScopes | Scenario::ScopeValidationFailure => {}
        }
        Ok(status)
    }

    fn indexed_scopes(&self) -> Result<Vec<IndexedScope>, SpikeError> {
        match self.0 {
            Scenario::EmptyScopes => Ok(Vec::new()),
            Scenario::ScopeValidationFailure => Err(SpikeError::not_runnable(
                "scope rules do not prove an indexed local root",
            )),
            Scenario::StoppedService | Scenario::MissingCatalog => {
                panic!("scope loading must not run after a failed health check")
            }
        }
    }

    fn query_literal(
        &self,
        _literal: &str,
        _limit: u32,
        _scopes: &[IndexedScope],
    ) -> Result<Vec<SearchHit>, SpikeError> {
        panic!("query construction must not run after a failed precondition")
    }
}

#[test]
fn failfast_preconditions_leave_all_query_counters_at_zero() {
    for scenario in [
        Scenario::StoppedService,
        Scenario::MissingCatalog,
        Scenario::EmptyScopes,
        Scenario::ScopeValidationFailure,
    ] {
        let error = execute_indexed_literal_query(&PanicOnQuery(scenario), "proof", 20)
            .expect_err("unprovable precondition must fail");
        assert_eq!(error.exit_code(), 2);
        assert_eq!(error.evidence().counters, Default::default());
    }
}
