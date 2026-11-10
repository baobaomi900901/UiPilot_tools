use super::lifecycle_macos::tests::{Temp, install, ID, REQUEST};
use std::{sync::Arc, time::{Instant, Duration}};
use serde_json::json;

#[test]
fn actual_result_registry_owner_is_retired_on_disable_and_prepared_result_cannot_publish() {
    use super::main_result_macos::MainResultPublisher;
    use crate::result_registry::{ResultRegistry, QueryDomain};
    use std::sync::Mutex;
    let t=Temp::new(); let mut host=t.host(); install(&mut host,t.package("1.0.0",false));
    let registry=ResultRegistry::default(); registry.on_show("invocation".into());
    let publisher=Arc::new(MainResultPublisher::new(registry.clone()));
    let owner_slot=Arc::new(Mutex::new(None));
    let p=publisher.clone(); let slot=owner_slot.clone();
    host.open_request(ID,"default",REQUEST,Instant::now()+Duration::from_secs(10), move |request| {
        let owner=Arc::new(p.begin(request.binding().clone(),"invocation",1,None).unwrap());
        *slot.lock().unwrap()=Some(owner.clone());
        Ok(Box::new(move||{assert!(p.retire(&owner));Ok(())}))
    }).unwrap();
    let owner=owner_slot.lock().unwrap().take().unwrap();
    let raw=serde_json::to_vec(&json!({"requestId":REQUEST,"results":[{"id":"row","title":"installed"}]})).unwrap();
    let prepared=publisher.prepare(&owner,&raw).unwrap();
    host.set_enabled(ID,false).unwrap();
    assert!(publisher.publish(&owner,prepared).is_err());
    assert!(registry.begin_query(QueryDomain::Plugin,"invocation",2).is_some());
}
#[test]
fn published_rows_retire_on_disable_corruption_and_drop_without_clearing_newer_query() {
    use super::main_result_macos::MainResultPublisher;
    use crate::result_registry::{QueryDomain, RegistryError, ResultAction, ResultRegistry};
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    };
    for transition in ["disable", "corrupt", "drop"] {
        for newer_query in [false, true] {
            let t = Temp::new();
            let mut host = t.host();
            install(&mut host, t.package("1.0.0", false));
            let registry = ResultRegistry::default();
            registry.on_show("invocation".into());
            let publisher = Arc::new(MainResultPublisher::new(registry.clone()));
            let owner_slot = Arc::new(Mutex::new(None));
            let cleanup_count = Arc::new(AtomicU64::new(0));
            let count = cleanup_count.clone();
            let p = publisher.clone();
            let slot = owner_slot.clone();
            host.open_request(
                ID,
                "default",
                REQUEST,
                Instant::now() + Duration::from_secs(10),
                move |request| {
                    let owner =
                        Arc::new(p.begin(request.binding().clone(), "invocation", 1, None).unwrap());
                    *slot.lock().unwrap() = Some(owner.clone());
                    Ok(Box::new(move || {
                        assert_eq!(p.retire(&owner), !newer_query);
                        count.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    }))
                },
            )
            .unwrap();
            let owner = owner_slot.lock().unwrap().take().unwrap();
            let raw = serde_json::to_vec(
                &json!({"requestId":REQUEST,"results":[{"id":"row","title":"published"}]}),
            )
            .unwrap();
            let prepared = publisher.prepare(&owner, &raw).unwrap();
            let response = publisher.publish(&owner, prepared).unwrap().unwrap();
            let row = &response.items[0].result_id;
            // Informational rows have a live result set but no action.
            assert_eq!(
                registry.resolve(&response.request_id, row),
                Err(RegistryError::UnknownResult)
            );
            let newer_id = if newer_query {
                let query = registry
                    .begin_query(QueryDomain::Application, "invocation", 2)
                    .unwrap();
                Some(
                    registry
                        .publish_if_latest(
                            query,
                            vec![((), None::<ResultAction>)],
                            || true,
                            |request_id, _| request_id,
                        )
                        .unwrap(),
                )
            } else {
                None
            };
            match transition {
                "disable" => {
                    assert_eq!(host.set_enabled(ID, false), Ok(true));
                }
                "corrupt" => {
                    t.corrupt_inventory();
                    assert!(host.set_enabled(ID, false).is_err());
                }
                "drop" => {}
                _ => unreachable!(),
            }
            drop(host);
            assert_eq!(cleanup_count.load(Ordering::SeqCst), 1);
            assert_eq!(
                registry.resolve(&response.request_id, row),
                Err(RegistryError::StaleRequest)
            );
            if let Some(newer_id) = newer_id {
                assert_eq!(
                    registry.resolve(&newer_id, "unknown"),
                    Err(RegistryError::UnknownResult)
                );
            }
            assert!(!publisher.retire(&owner));
        }
    }
}
