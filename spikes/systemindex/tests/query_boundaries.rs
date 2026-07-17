use std::cell::RefCell;

use systemindex_spike::{
    IndexedScope, OperationCounters, QueryOperations, SearchBackend, SearchHit, SearchStatus,
    SpikeError, execute_indexed_literal_query, run_query_operations,
};

#[derive(Default)]
struct RecordingOperations {
    events: Vec<String>,
    literal: Option<String>,
    enumeration_limit: Option<u32>,
}

impl QueryOperations for RecordingOperations {
    fn create_condition_leaf(&mut self, literal: &str) -> Result<(), SpikeError> {
        self.events.push("create condition leaf".to_owned());
        self.literal = Some(literal.to_owned());
        Ok(())
    }

    fn create_search_folder_factory(&mut self) -> Result<(), SpikeError> {
        self.events.push("create Search Folder factory".to_owned());
        Ok(())
    }

    fn set_condition(&mut self) -> Result<(), SpikeError> {
        self.events.push("set condition".to_owned());
        Ok(())
    }

    fn set_display_name(&mut self) -> Result<(), SpikeError> {
        self.events.push("set display name".to_owned());
        Ok(())
    }

    fn set_explicit_scopes(&mut self, scopes: &[IndexedScope]) -> Result<(), SpikeError> {
        assert_eq!(
            scopes,
            [IndexedScope {
                url: "file:///C:/Users/".to_owned()
            }]
        );
        self.events.push("set explicit scopes".to_owned());
        Ok(())
    }

    fn get_shell_item(&mut self) -> Result<(), SpikeError> {
        self.events.push("get shell item".to_owned());
        Ok(())
    }

    fn enumerate(&mut self, limit: u32) -> Result<Vec<SearchHit>, SpikeError> {
        self.events.push("enumerate".to_owned());
        self.enumeration_limit = Some(limit);
        Ok(vec![SearchHit {
            display_name: "report.txt".to_owned(),
            parsing_path: r"C:\Users\report.txt".to_owned(),
        }])
    }
}

struct RecordingBackend {
    events: RefCell<Vec<String>>,
    operations: RefCell<RecordingOperations>,
}

impl RecordingBackend {
    fn new() -> Self {
        Self {
            events: RefCell::new(Vec::new()),
            operations: RefCell::new(RecordingOperations::default()),
        }
    }
}

impl SearchBackend for RecordingBackend {
    fn status(&self) -> Result<SearchStatus, SpikeError> {
        self.events
            .borrow_mut()
            .push("check service/catalog".to_owned());
        Ok(SearchStatus {
            catalog: "SystemIndex".to_owned(),
            service_running: true,
            catalog_available: true,
        })
    }

    fn indexed_scopes(&self) -> Result<Vec<IndexedScope>, SpikeError> {
        self.events
            .borrow_mut()
            .push("load validated scopes".to_owned());
        Ok(vec![IndexedScope {
            url: "file:///C:/Users/".to_owned(),
        }])
    }

    fn query_literal(
        &self,
        literal: &str,
        limit: u32,
        scopes: &[IndexedScope],
    ) -> Result<Vec<SearchHit>, SpikeError> {
        let mut operations = self.operations.borrow_mut();
        let items = run_query_operations(&mut *operations, literal, limit, scopes)?;
        self.events.borrow_mut().extend(operations.events.clone());
        Ok(items)
    }
}

#[test]
fn passes_special_characters_unchanged_to_the_typed_leaf() {
    for literal in [
        "'single'",
        "\"double\"",
        "*?%_[]",
        r"back\slash",
        " leading and trailing ",
        "文件名",
        "emoji😀",
        "é",
        "e\u{301}",
    ] {
        let backend = RecordingBackend::new();
        execute_indexed_literal_query(&backend, literal, 20).unwrap();
        assert_eq!(
            backend.operations.borrow().literal.as_deref(),
            Some(literal)
        );
    }
}

#[test]
fn rejects_more_than_256_unicode_scalars_at_the_execution_boundary() {
    let backend = RecordingBackend::new();
    assert!(execute_indexed_literal_query(&backend, &"界".repeat(256), 1).is_ok());

    let backend = RecordingBackend::new();
    assert!(execute_indexed_literal_query(&backend, &"界".repeat(257), 1).is_err());
    assert!(backend.events.borrow().is_empty());
}

#[test]
fn executes_preconditions_and_structured_query_in_the_required_order() {
    let backend = RecordingBackend::new();

    let evidence = execute_indexed_literal_query(&backend, "report", 7).unwrap();

    assert_eq!(
        backend.events.borrow().as_slice(),
        [
            "check service/catalog",
            "load validated scopes",
            "create condition leaf",
            "create Search Folder factory",
            "set condition",
            "set display name",
            "set explicit scopes",
            "get shell item",
            "enumerate",
        ]
    );
    assert_eq!(backend.operations.borrow().enumeration_limit, Some(7));
    assert_eq!(evidence.items.len(), 1);
    assert_eq!(
        evidence.counters,
        OperationCounters {
            search_folder_factory_created: 1,
            scope_set: 1,
            search_folder_enumerated: 1,
        }
    );
}
