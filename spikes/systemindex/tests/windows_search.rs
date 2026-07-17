#![cfg(windows)]

use systemindex_spike::WindowsSearch;

#[test]
fn reads_systemindex_status_and_scopes_without_query_work() {
    let search = WindowsSearch::connect().unwrap();
    let status = search.status().unwrap();
    assert_eq!(status.catalog, "SystemIndex");
    assert!(status.service_running);
    assert!(status.catalog_available);

    let evidence = search.scope_evidence().unwrap();
    assert!(!evidence.included_file_roots.is_empty());
    assert!(
        evidence
            .included_file_roots
            .iter()
            .all(|scope| scope.to_ascii_lowercase().starts_with("file:///"))
    );
    assert_eq!(evidence.counters.search_folder_factory_created, 0);
    assert_eq!(evidence.counters.scope_set, 0);
    assert_eq!(evidence.counters.search_folder_enumerated, 0);
}
