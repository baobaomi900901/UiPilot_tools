use systemindex_spike::{CrawlRule, OperationCounters, SearchStatus, validated_file_scopes};

fn healthy_status() -> SearchStatus {
    SearchStatus {
        catalog: "SystemIndex".to_owned(),
        service_running: true,
        catalog_available: true,
    }
}

#[test]
fn selects_only_local_included_file_directory_rules() {
    let rules = vec![
        CrawlRule::included(r"file:///C:\Users\Ada\Documents\"),
        CrawlRule::excluded(r"file:///C:\Users\Ada\Documents\Private\"),
        CrawlRule::included("file:///D:/Work/"),
        CrawlRule::included(r"file:///C:\Users\Ada\*.tmp"),
        CrawlRule::included("file://server/share/"),
        CrawlRule::included(r"C:\"),
        CrawlRule::included("mapi://mailbox/"),
        CrawlRule::included("文件协议不是file目录规则"),
    ];

    let scopes = validated_file_scopes(&healthy_status(), rules).unwrap();
    assert_eq!(
        scopes
            .iter()
            .map(|scope| scope.url.as_str())
            .collect::<Vec<_>>(),
        vec![r"file:///C:\Users\Ada\Documents\", "file:///D:/Work/"]
    );
}

#[test]
fn fails_before_query_work_when_search_preconditions_are_unprovable() {
    for status in [
        SearchStatus {
            service_running: false,
            ..healthy_status()
        },
        SearchStatus {
            catalog_available: false,
            ..healthy_status()
        },
    ] {
        assert!(
            validated_file_scopes(
                &status,
                vec![CrawlRule::included(r"file:///C:\Users\Ada\Documents\")],
            )
            .is_err()
        );
    }

    assert!(validated_file_scopes(&healthy_status(), Vec::new()).is_err());
    assert!(
        validated_file_scopes(
            &healthy_status(),
            vec![CrawlRule::included("file://server/share/")],
        )
        .is_err()
    );
}

#[test]
fn operation_counters_start_at_zero_and_serialize_with_stable_names() {
    let counters = OperationCounters::default();
    assert_eq!(counters.search_folder_factory_created, 0);
    assert_eq!(counters.scope_set, 0);
    assert_eq!(counters.search_folder_enumerated, 0);

    assert_eq!(
        serde_json::to_value(counters).unwrap(),
        serde_json::json!({
            "searchFolderFactoryCreated": 0,
            "scopeSet": 0,
            "searchFolderEnumerated": 0,
        })
    );
}
