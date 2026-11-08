use super::super::runtime_result_macos::tests::{raw, Fixture};
use super::*;
use crate::result_registry::{RegistryError, ResultRegistries};
use serde_json::json;
fn setup(registry: &ResultRegistry, fixture: &Fixture) -> (MainResultPublisher, MainResultOwner) {
    registry.on_show("invocation".into());
    let publisher = MainResultPublisher::new(registry.clone());
    let owner = publisher
        .begin(fixture.binding.clone(), "invocation", 1)
        .unwrap();
    (publisher, owner)
}
#[test]
fn actual_main_registry_publishes_opaque_informational_rows_without_execution_payload() {
    let f = Fixture::new();
    let registries = ResultRegistries::default();
    let registry = registries.main();
    let (publisher, owner) = setup(registry, &f);
    let publication = publisher.prepare(&owner, &raw()).unwrap();
    let response = publisher.publish(&owner, publication).unwrap().unwrap();
    assert_eq!(response.items.len(), 1);
    assert_eq!(response.items[0].title, "中文 😀");
    let wire = serde_json::to_value(&response).unwrap();
    assert_eq!(
        wire["items"][0]["activation"],
        json!({"kind":"informational"})
    );
    assert!(!response.items[0].has_default_action);
    let text = wire.to_string();
    for private in [
        "plugin-private-row-id",
        "copyText",
        "defaultAction",
        "com.uipilot.fixture",
    ] {
        assert!(!text.contains(private));
    }
    assert_eq!(
        registry.resolve(&response.request_id, &response.items[0].result_id),
        Err(RegistryError::UnknownResult)
    );
    assert!(!publisher.fail(&owner)); // late error must not retire successful query
    assert!(publisher.retire(&owner)); // trusted lifecycle can still retire it
    assert!(!publisher.retire(&owner));
    let q = registry
        .begin_query(QueryDomain::Plugin, "invocation", 2)
        .unwrap();
    assert!(registry
        .publish_if_latest(q, vec![(1, None::<ResultAction>)], || true, |_, v| v)
        .is_some());
}
#[test]
fn equal_numeric_queries_in_two_registries_do_not_transfer_owner_or_publication() {
    let a = Fixture::new();
    let b = Fixture::new();
    let ra = ResultRegistry::default();
    let rb = ResultRegistry::default();
    let (pa, oa) = setup(&ra, &a);
    let (pb, ob) = setup(&rb, &b);
    assert!(matches!(
        pb.prepare(&oa, &raw()),
        Err(MainCompletionError::Expired)
    ));
    assert!(!pb.fail(&oa));
    assert!(!pb.retire(&oa));
    let ticket = pa.prepare(&oa, &raw()).unwrap();
    assert!(matches!(
        pb.publish(&ob, ticket),
        Err(MainCompletionError::Expired)
    ));
    let ticket = pb.prepare(&ob, &raw()).unwrap();
    assert!(pb.publish(&ob, ticket).unwrap().is_some());
}
#[test]
fn newer_application_query_survives_old_plugin_publication_and_cleanup() {
    let f = Fixture::new();
    let registry = ResultRegistry::default();
    let (publisher, owner) = setup(&registry, &f);
    let publication = publisher.prepare(&owner, &raw()).unwrap();
    let q = registry
        .begin_query(QueryDomain::Application, "invocation", 2)
        .unwrap();
    let action = ResultAction::CopyBuiltInText {
        text: "fresh".into(),
    };
    let (request, items) = registry
        .publish_if_latest(
            q,
            vec![("fresh", action.clone())],
            || true,
            |id, rows| (id, rows),
        )
        .unwrap();
    assert!(publisher.publish(&owner, publication).unwrap().is_none());
    assert!(!publisher.fail(&owner));
    assert!(!publisher.retire(&owner));
    assert_eq!(registry.resolve(&request, &items[0].0), Ok(action));
}
#[test]
fn pending_failure_is_single_use_and_cannot_publish_prepared_results() {
    let f = Fixture::new();
    let registry = ResultRegistry::default();
    let (publisher, owner) = setup(&registry, &f);
    let publication = publisher.prepare(&owner, &raw()).unwrap();
    assert!(publisher.fail(&owner));
    assert!(!publisher.fail(&owner));
    assert!(matches!(
        publisher.publish(&owner, publication),
        Err(MainCompletionError::Expired)
    ));
    assert!(publisher.prepare(&owner, &raw()).is_err());
}
#[test]
fn hide_show_and_lifecycle_retirement_do_not_erase_a_new_query() {
    let f = Fixture::new();
    let registry = ResultRegistry::default();
    let (publisher, owner) = setup(&registry, &f);
    let publication = publisher.prepare(&owner, &raw()).unwrap();
    registry.hide_and_clear();
    registry.on_show("new-invocation".into());
    let q = registry
        .begin_query(QueryDomain::Application, "new-invocation", 1)
        .unwrap();
    let action = ResultAction::CopyBuiltInText {
        text: "new-window".into(),
    };
    let (request, rows) = registry
        .publish_if_latest(
            q,
            vec![((), action.clone())],
            || true,
            |id, rows| (id, rows),
        )
        .unwrap();
    assert!(!publisher.retire(&owner));
    assert!(publisher.publish(&owner, publication).is_err());
    assert_eq!(registry.resolve(&request, &rows[0].0), Ok(action));
}
#[test]
fn find_scope_and_invalid_results_have_no_informational_fallback() {
    let f = Fixture::new();
    let registries = ResultRegistries::default();
    registries.find().on_show("find".into());
    assert!(MainResultPublisher::new(registries.find().clone())
        .begin(f.binding.clone(), "find", 1)
        .is_none());
    let (publisher, owner) = setup(registries.main(), &f);
    let mut value: serde_json::Value = serde_json::from_slice(&raw()).unwrap();
    value["results"][0]["defaultAction"] = json!({"type":"copyText","text":"private-copy"});
    assert!(matches!(
        publisher.prepare(&owner, &serde_json::to_vec(&value).unwrap()),
        Err(MainCompletionError::UnsupportedAction)
    ));
    value["results"][0]["unknown"] = json!(true);
    assert!(matches!(
        publisher.prepare(&owner, &serde_json::to_vec(&value).unwrap()),
        Err(MainCompletionError::Expired)
    ));
    assert!(matches!(
        publisher.prepare(&owner, &raw()),
        Err(MainCompletionError::Expired)
    ));
    assert!(!publisher.fail(&owner));
}

#[test]
fn admitted_old_publication_cannot_overwrite_new_plugin_query() {
    use std::{sync::mpsc, thread, time::Duration};
    let f = Fixture::new();
    let registry = ResultRegistry::default();
    let (publisher, owner) = setup(&registry, &f);
    let publication = publisher.prepare(&owner, &raw()).unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    *publisher.before_publish.lock().unwrap() = Some(Box::new(move || {
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    }));
    let publisher = Arc::new(publisher);
    let worker_publisher = publisher.clone();
    let owner = Arc::new(owner);
    let worker_owner = owner.clone();
    let worker = thread::spawn(move || worker_publisher.publish(&worker_owner, publication));
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let query = registry
        .begin_query(QueryDomain::Plugin, "invocation", 2)
        .unwrap();
    let action = ResultAction::CopyBuiltInText {
        text: "new-plugin-marker".into(),
    };
    let (request, rows) = registry
        .publish_if_latest(
            query,
            vec![((), action.clone())],
            || true,
            |id, rows| (id, rows),
        )
        .unwrap();
    release_tx.send(()).unwrap();
    assert!(worker.join().unwrap().unwrap().is_none());
    assert_eq!(registry.resolve(&request, &rows[0].0), Ok(action));
    assert!(!publisher.fail(&owner));
    assert!(!publisher.retire(&owner));
}
