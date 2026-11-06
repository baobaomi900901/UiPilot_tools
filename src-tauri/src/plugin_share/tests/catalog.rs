use super::*;

#[test]
fn catalog_is_stable_searchable_paged_and_retains_size_disabled_targets() {
    let mut receivers: Vec<_> = (0..1200)
        .rev()
        .map(|i| receiver(&format!("plugin.{i:04}")))
        .collect();
    receivers.push(receiver("source"));
    receivers[0].max_bytes = 1;
    let mut offset = 0;
    let mut ids = vec![];
    let mut disabled = 0;
    loop {
        let page = discover(&receivers, "source", 10, "", offset).unwrap();
        assert!(compact_envelope(&page).unwrap().len() <= ENVELOPE_LIMIT);
        disabled += page
            .items
            .iter()
            .filter(|i| i.disabled_reason.is_some())
            .count();
        ids.extend(
            page.items
                .iter()
                .map(|i| i.receiver.plugin.plugin_id.clone()),
        );
        match page.next_offset {
            Some(next) => {
                assert!(next > offset);
                offset = next;
            }
            None => break,
        }
    }
    assert_eq!(ids.len(), 1200);
    assert_eq!(disabled, 1);
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
    let page = discover(&receivers, "source", 10, "plugin.001", 0).unwrap();
    assert_eq!(page.items.len(), 10);
}

#[cfg(windows)]
#[test]
fn secure_ids_use_os_entropy_with_fixed_size_tokens() {
    let mut ids = SecureIds;
    let a = ids.next().unwrap();
    let b = ids.next().unwrap();
    assert_eq!(a.0.len(), 64);
    assert!(a.0.bytes().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(a, b);
}

#[test]
fn catalog_roundtrips_large_plugin_identity_without_javascript_number_loss() {
    let mut plugin = doc("target").plugin;
    plugin.inventory_revision = u64::MAX;
    let encoded = serde_json::to_value(&plugin).unwrap();
    assert_eq!(encoded["inventoryRevision"], json!(u64::MAX.to_string()));
    assert_eq!(
        serde_json::from_value::<PluginIdentity>(encoded).unwrap(),
        plugin
    );
}
