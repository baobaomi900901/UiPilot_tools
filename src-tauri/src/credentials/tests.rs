use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEST: AtomicU64 = AtomicU64::new(0);
struct TestStore {
    store: CredentialStore,
    root: std::path::PathBuf,
}
impl TestStore {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "uipilot-credential-tests-{}-{}",
            std::process::id(),
            NEXT_TEST.fetch_add(1, Ordering::Relaxed)
        ));
        Self {
            store: CredentialStore::new(root.clone()),
            root,
        }
    }
}
impl Drop for TestStore {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn mutation(service: Service, revision: &str) -> MutationInput {
    MutationInput {
        service,
        expected_revision: revision.into(),
    }
}
fn save(revision: &str, secret: &str) -> SaveInput {
    SaveInput {
        service: Service::Deepseek,
        expected_revision: revision.into(),
        name: "Synthetic credential".into(),
        values: [("apiKey".into(), secret.into())].into(),
    }
}
#[test]
fn management_rejects_every_non_main_caller() {
    assert!(require_main_label("main").is_ok());
    for label in ["", "find", "plugin-demo", "Main"] {
        assert_eq!(require_main_label(label).unwrap_err().code, "invalidCaller");
    }
}
#[test]
fn add_persists_disabled_metadata_and_refuses_duplicates_and_stale_revisions() {
    let fixture = TestStore::new();
    assert_eq!(fixture.store.list().unwrap().revision, "0");
    let added = fixture.store.add(mutation(Service::Deepseek, "0")).unwrap();
    assert_eq!(added.revision, "1");
    assert!(!added.items[0].configured);
    assert!(!added.items[0].enabled);
    assert_eq!(
        fixture
            .store
            .add(mutation(Service::Deepseek, "1"))
            .unwrap_err()
            .code,
        "invalidInput"
    );
    assert_eq!(
        fixture
            .store
            .add(mutation(Service::BaiduOcr, "0"))
            .unwrap_err()
            .code,
        "staleInventory"
    );
    assert_eq!(
        fixture
            .store
            .set_enabled(EnabledInput {
                service: Service::Deepseek,
                enabled: true,
                expected_revision: "1".into()
            })
            .unwrap_err()
            .code,
        "notConfigured"
    );
    assert_eq!(
        CredentialStore::new(fixture.root.clone()).list().unwrap(),
        added
    );
}
#[tokio::test]
async fn validation_success_persists_encrypted_values_and_metadata_never_exposes_them() {
    let fixture = TestStore::new();
    fixture.store.add(mutation(Service::Deepseek, "0")).unwrap();
    let result = fixture
        .store
        .save_with(save("1", "synthetic-secret-value"), |record| async move {
            assert_eq!(record.values["apiKey"], "synthetic-secret-value");
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(result.revision, "2");
    assert!(result.items[0].configured);
    assert!(!result.items[0].enabled);
    let dto = serde_json::to_string(&result).unwrap();
    assert!(!dto.contains("apiKey"));
    assert!(!dto.contains("synthetic-secret-value"));
    let disk = std::fs::read(fixture.root.join(store::FILE_NAME)).unwrap();
    assert!(!disk
        .windows(b"synthetic-secret-value".len())
        .any(|value| value == b"synthetic-secret-value"));
    assert!(!disk
        .windows(b"Synthetic credential".len())
        .any(|value| value == b"Synthetic credential"));
    assert_eq!(
        CredentialStore::new(fixture.root.clone()).list().unwrap(),
        result
    );
    let enabled = fixture
        .store
        .set_enabled(EnabledInput {
            service: Service::Deepseek,
            enabled: true,
            expected_revision: "2".into(),
        })
        .unwrap();
    assert!(enabled.items[0].enabled);
    let retained = fixture
        .store
        .save_with(save("3", ""), |record| async move {
            assert_eq!(record.values["apiKey"], "synthetic-secret-value");
            Ok(())
        })
        .await
        .unwrap();
    assert!(retained.items[0].enabled);
}
#[tokio::test]
async fn failed_validation_preserves_old_revision_and_values() {
    let fixture = TestStore::new();
    fixture.store.add(mutation(Service::Deepseek, "0")).unwrap();
    let before = fixture
        .store
        .save_with(save("1", "old-synthetic"), |_| async { Ok(()) })
        .await
        .unwrap();
    let bytes = std::fs::read(fixture.root.join(store::FILE_NAME)).unwrap();
    let error = fixture
        .store
        .save_with(save("2", "new-synthetic"), |_| async {
            Err(Error::invalid_credentials())
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, "invalidCredentials");
    assert_eq!(fixture.store.list().unwrap(), before);
    assert_eq!(
        std::fs::read(fixture.root.join(store::FILE_NAME)).unwrap(),
        bytes
    );
}
#[tokio::test]
async fn deletion_and_readdition_during_validation_cannot_resurrect_or_overwrite() {
    let fixture = TestStore::new();
    fixture.store.add(mutation(Service::Deepseek, "0")).unwrap();
    let error = fixture
        .store
        .save_with(save("1", "old-request-synthetic"), |_| async {
            fixture
                .store
                .delete(mutation(Service::Deepseek, "1"))
                .unwrap();
            fixture.store.add(mutation(Service::Deepseek, "2")).unwrap();
            Ok(())
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, "staleInventory");
    let current = fixture.store.list().unwrap();
    assert_eq!(current.revision, "3");
    assert!(!current.items[0].configured);
}
#[tokio::test]
async fn concurrent_enable_invalidates_pending_save() {
    let fixture = TestStore::new();
    fixture.store.add(mutation(Service::Deepseek, "0")).unwrap();
    fixture
        .store
        .save_with(save("1", "first-synthetic"), |_| async { Ok(()) })
        .await
        .unwrap();
    let error = fixture
        .store
        .save_with(save("2", "new-synthetic"), |_| async {
            fixture
                .store
                .set_enabled(EnabledInput {
                    service: Service::Deepseek,
                    enabled: true,
                    expected_revision: "2".into(),
                })
                .unwrap();
            Ok(())
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, "staleInventory");
    assert!(fixture.store.list().unwrap().items[0].enabled);
}
#[tokio::test]
async fn unknown_fields_incomplete_values_and_invalid_names_fail_before_transport() {
    let fixture = TestStore::new();
    fixture.store.add(mutation(Service::Deepseek, "0")).unwrap();
    let mut unknown = save("1", "synthetic");
    unknown
        .values
        .insert("baseUrl".into(), "https://example.invalid".into());
    let mut long_name = save("1", "synthetic");
    long_name.name = "密".repeat(81);
    for input in [unknown, long_name, save("1", "")] {
        assert_eq!(
            fixture
                .store
                .save_with(input, |_| async { panic!("transport must not run") })
                .await
                .unwrap_err()
                .code,
            "invalidInput"
        );
    }
    assert!(parse_input::<MutationInput>(
        serde_json::json!({"service":"custom", "expectedRevision":"1"})
    )
    .is_err());
    assert!(parse_input::<MutationInput>(
        serde_json::json!({"service":"deepseek", "expectedRevision":"1", "secret":"synthetic"})
    )
    .is_err());
    assert_eq!(
        fixture
            .store
            .delete(mutation(Service::Deepseek, "01"))
            .unwrap_err()
            .code,
        "invalidInput"
    );
}
#[test]
fn corruption_fails_closed_and_backup_is_never_recovered() {
    let fixture = TestStore::new();
    fixture.store.add(mutation(Service::Deepseek, "0")).unwrap();
    let file = fixture.root.join(store::FILE_NAME);
    std::fs::copy(&file, file.with_extension("backup")).unwrap();
    std::fs::write(&file, b"synthetic-invalid-ciphertext").unwrap();
    let reopened = CredentialStore::new(fixture.root.clone());
    assert_eq!(reopened.list().unwrap_err().code, "storageFailed");
    assert_eq!(
        reopened
            .add(mutation(Service::BaiduOcr, "0"))
            .unwrap_err()
            .code,
        "storageFailed"
    );
    assert_eq!(
        std::fs::read(file).unwrap(),
        b"synthetic-invalid-ciphertext"
    );
}
#[tokio::test]
async fn failed_atomic_write_does_not_publish_candidate_in_memory() {
    let fixture = TestStore::new();
    fixture.store.add(mutation(Service::Deepseek, "0")).unwrap();
    let before = fixture.store.list().unwrap();
    std::fs::remove_file(fixture.root.join(store::FILE_NAME)).unwrap();
    std::fs::create_dir(fixture.root.join(store::FILE_NAME)).unwrap();
    assert_eq!(
        fixture
            .store
            .save_with(save("1", "synthetic"), |_| async { Ok(()) })
            .await
            .unwrap_err()
            .code,
        "storageFailed"
    );
    assert_eq!(fixture.store.list().unwrap(), before);
}

#[test]
fn credential_commands_are_registered_once_and_only_main_has_permissions() {
    let lib = include_str!("../lib.rs");
    let production = lib.split("#[cfg(test)]\nmod tests").next().unwrap();
    let build = include_str!("../../build.rs");
    let main: serde_json::Value =
        serde_json::from_str(include_str!("../../capabilities/main.json")).unwrap();
    let source = include_str!("mod.rs");
    let capabilities = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities");
    for command in [
        "list_credentials",
        "add_credential",
        "save_credential",
        "set_credential_enabled",
        "delete_credential",
    ] {
        assert_eq!(
            production
                .matches(&format!("credentials::{command},"))
                .count(),
            1
        );
        assert_eq!(build.matches(&format!("\"{command}\",")).count(), 1);
        let permission = format!("allow-{}", command.replace('_', "-"));
        assert_eq!(
            main["permissions"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entry| entry.as_str() == Some(&permission))
                .count(),
            1
        );
        for file in std::fs::read_dir(&capabilities).unwrap().flatten() {
            if file.file_name() == "main.json"
                || file
                    .path()
                    .extension()
                    .is_none_or(|extension| extension != "json")
            {
                continue;
            }
            assert!(!std::fs::read_to_string(file.path())
                .unwrap()
                .contains(&permission));
        }
        let function = source
            .split(&format!("fn {command}("))
            .nth(1)
            .unwrap()
            .split("\n}")
            .next()
            .unwrap();
        assert!(function.contains("require_main_label(window.label())?;"));
    }
}

#[test]
fn encrypted_document_cannot_be_rebound_to_a_different_storage_directory() {
    let original = TestStore::new();
    original
        .store
        .add(mutation(Service::Deepseek, "0"))
        .unwrap();
    let other = TestStore::new();
    std::fs::create_dir_all(&other.root).unwrap();
    std::fs::copy(
        original.root.join(store::FILE_NAME),
        other.root.join(store::FILE_NAME),
    )
    .unwrap();
    assert_eq!(other.store.list().unwrap_err().code, "storageFailed");
}

#[tokio::test]
async fn all_services_accept_only_their_complete_field_sets() {
    let fixture = TestStore::new();
    let mut revision = "0".to_string();
    for service in [
        Service::YoudaoTranslation,
        Service::BaiduTranslation,
        Service::BaiduOcr,
        Service::Deepseek,
    ] {
        revision = fixture
            .store
            .add(mutation(service, &revision))
            .unwrap()
            .revision;
        let input = SaveInput {
            service,
            name: "Synthetic service".into(),
            expected_revision: revision,
            values: service
                .fields()
                .iter()
                .map(|field| ((*field).to_string(), format!("synthetic-{field}")))
                .collect(),
        };
        let result = fixture
            .store
            .save_with(input, |record| async move {
                assert!(record.configured());
                assert_eq!(record.values.len(), service.fields().len());
                Ok(())
            })
            .await
            .unwrap();
        revision = result.revision;
        assert!(result
            .items
            .iter()
            .all(|entry| entry.configured && !entry.enabled));
    }
    let reopened = CredentialStore::new(fixture.root.clone()).list().unwrap();
    assert_eq!(reopened.revision, "8");
    assert_eq!(reopened.items.len(), 4);
}

#[test]
fn host_service_lease_is_invalidated_before_revision_can_be_delivered_and_never_revives() {
    let fixture = TestStore::new();
    assert_eq!(
        fixture.store.with_youdao(
            &crate::public_plugins::PluginRequestContext {
                plugin_id: "com.example.test".into(),
                plugin_generation: 1,
                request_id: "synthetic-request".into()
            },
            crate::public_plugins::result_action::ResultActionLease::default(),
            Box::new(|| {}),
            |_, _| Ok(())
        ),
        Err("credentialNotConfigured")
    );
    fixture.store.synthetic_youdao_for_test();
    let retired = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let witness = retired.clone();
    let revision = fixture
        .store
        .with_youdao(
            &crate::public_plugins::PluginRequestContext {
                plugin_id: "com.example.test".into(),
                plugin_generation: 1,
                request_id: "synthetic-request".into(),
            },
            crate::public_plugins::result_action::ResultActionLease::default(),
            Box::new(move || {
                witness.store(true, Ordering::SeqCst);
            }),
            |revision, _| Ok(revision),
        )
        .unwrap();
    assert_eq!(
        fixture.store.with_revision(revision, || Ok("delivered")),
        Ok("delivered")
    );
    fixture.store.disable_youdao_for_test();
    assert!(retired.load(Ordering::SeqCst));
    assert_eq!(
        fixture.store.with_revision(revision, || Ok(())),
        Err("expiredRequest")
    );
    assert_eq!(
        fixture.store.with_youdao(
            &crate::public_plugins::PluginRequestContext {
                plugin_id: "com.example.test".into(),
                plugin_generation: 1,
                request_id: "synthetic-request".into()
            },
            crate::public_plugins::result_action::ResultActionLease::default(),
            Box::new(|| {}),
            |_, _| Ok(())
        ),
        Err("credentialDisabled")
    );
    fixture
        .store
        .delete(mutation(Service::YoudaoTranslation, "2"))
        .unwrap();
    fixture
        .store
        .add(mutation(Service::YoudaoTranslation, "3"))
        .unwrap();
    assert_eq!(
        fixture.store.with_revision(revision, || Ok(())),
        Err("expiredRequest")
    );
}

#[test]
fn host_service_rejected_admission_does_not_replace_native_completion_revision() {
    let fixture = TestStore::new();
    fixture.store.synthetic_youdao_for_test();
    let context = crate::public_plugins::PluginRequestContext {
        plugin_id: "com.example.test".into(),
        plugin_generation: 1,
        request_id: "admitted".into(),
    };
    fixture
        .store
        .with_youdao(
            &context,
            crate::public_plugins::result_action::ResultActionLease::default(),
            Box::new(|| {}),
            |_, _| Ok(()),
        )
        .unwrap();
    let forged = crate::public_plugins::PluginRequestContext {
        request_id: "forged".into(),
        ..context.clone()
    };
    assert_eq!(
        fixture.store.with_youdao(
            &forged,
            crate::public_plugins::result_action::ResultActionLease::default(),
            Box::new(|| {}),
            |_, _| Err::<(), _>("expiredRequest")
        ),
        Err("expiredRequest")
    );
    assert_eq!(
        fixture
            .store
            .with_completion(&context, |revision| revision.map(|(r, _)| r))
            .unwrap(),
        Some(1)
    );
    assert_eq!(
        fixture
            .store
            .with_completion(&forged, |revision| revision)
            .unwrap(),
        None
    );
    fixture.store.disable_youdao_for_test();
    assert_eq!(
        fixture
            .store
            .with_completion(&context, |revision| revision.map(|(r, _)| r))
            .unwrap(),
        Some(1)
    );
}
