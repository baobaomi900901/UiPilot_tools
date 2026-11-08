use super::*;
use serde_json::json;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    sync::mpsc,
    thread,
    time::Duration,
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = PathBuf::from(format!(
            "/private/tmp/uipilot-runtime-data-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["storage", "settings"] {
            fs::create_dir(path.join(name)).unwrap();
            fs::set_permissions(path.join(name), fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self(path)
    }
    fn host(&self) -> RuntimeDataHost {
        RuntimeDataHost::load(&self.0.join("storage"), &self.0.join("settings")).unwrap()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn context() -> PluginRequestContext {
    PluginRequestContext {
        plugin_id: "com.uipilot.fixture".into(),
        plugin_generation: 1,
        request_id: "public-request-0000000000000001".into(),
    }
}
fn definitions() -> Vec<PublicSettingV1> {
    serde_json::from_value(json!([
        {"type":"text","key":"title","label":"Title","default":"default"},
        {"type":"text","key":"optional","label":"Optional"},
        {"type":"secret","key":"secret","label":"Secret"}
    ]))
    .unwrap()
}
fn open(host: &mut RuntimeDataHost, title: &str) -> RuntimeDataBinding {
    host.open_request(
        context(),
        &definitions(),
        &BTreeMap::from([("title".into(), json!(title))]),
        Instant::now() + Duration::from_secs(5),
        || Ok(()),
    )
    .unwrap()
}
fn call(binding: &RuntimeDataBinding, id: u64, request: DataOperation) -> Result<Value> {
    let reply = binding.call(DataCall {
        context: binding.context.clone(),
        call_id: id,
        request,
    })?;
    reply.result
}
#[test]
fn real_backends_persist_then_host_reopens_and_setting_reads_are_declared_only() {
    let temp = Temp::new();
    let mut host = temp.host();
    let binding = open(&mut host, "中文 😀");
    assert_eq!(
        call(
            &binding,
            1,
            DataOperation::SettingGet {
                key: "title".into()
            }
        ),
        Ok(json!("中文 😀"))
    );
    assert_eq!(
        call(
            &binding,
            2,
            DataOperation::SettingGet {
                key: "optional".into()
            }
        ),
        Ok(Value::Null)
    );
    for (id, key) in [(3, "secret"), (4, "unknown")] {
        assert_eq!(
            call(&binding, id, DataOperation::SettingGet { key: key.into() }),
            Err(DataRuntimeError::InvalidOperation)
        );
    }
    assert_eq!(
        call(
            &binding,
            5,
            DataOperation::StorageSet {
                key: "sample".into(),
                value: json!([false, 0, null, "😀"])
            }
        ),
        Ok(Value::Null)
    );
    host.close(|| Ok(())).unwrap();
    drop(binding);
    drop(host);
    let mut host = temp.host();
    let binding = host
        .open_request(
            context(),
            &definitions(),
            &BTreeMap::new(),
            Instant::now() + Duration::from_secs(5),
            || Ok(()),
        )
        .unwrap();
    assert_eq!(
        call(
            &binding,
            1,
            DataOperation::SettingGet {
                key: "title".into()
            }
        ),
        Ok(json!("中文 😀"))
    );
    assert_eq!(
        call(
            &binding,
            2,
            DataOperation::StorageGet {
                key: "sample".into()
            }
        ),
        Ok(json!([false, 0, null, "😀"]))
    );
    assert_eq!(
        call(
            &binding,
            3,
            DataOperation::StorageRemove {
                key: "sample".into()
            }
        ),
        Ok(Value::Null)
    );
    assert_eq!(
        call(
            &binding,
            4,
            DataOperation::StorageGet {
                key: "sample".into()
            }
        ),
        Ok(Value::Null)
    );
}
#[test]
fn forged_context_and_duplicate_call_never_select_another_owner() {
    let temp = Temp::new();
    let mut host = temp.host();
    let binding = open(&mut host, "first");
    let mut wrong = context();
    wrong.plugin_id = "com.uipilot.other".into();
    assert!(binding
        .call(DataCall {
            context: wrong,
            call_id: 1,
            request: DataOperation::StorageSet {
                key: "sample".into(),
                value: json!(1)
            }
        })
        .is_err());
    assert!(!temp.0.join("storage/com.uipilot.other").exists());
    assert!(!temp.0.join("storage/com.uipilot.fixture").exists());
    assert_eq!(
        call(
            &binding,
            1,
            DataOperation::StorageSet {
                key: "sample".into(),
                value: json!(1)
            }
        ),
        Ok(Value::Null)
    );
    assert_eq!(
        call(
            &binding,
            1,
            DataOperation::StorageSet {
                key: "sample".into(),
                value: json!(2)
            }
        ),
        Err(DataRuntimeError::InvalidContext)
    );
    assert_eq!(
        call(
            &binding,
            2,
            DataOperation::StorageGet {
                key: "sample".into()
            }
        ),
        Ok(json!(1))
    );
}
#[test]
fn replacement_cancels_before_drain_and_cannot_publish_until_old_reply_releases() {
    let temp = Temp::new();
    let mut host = temp.host();
    let old = open(&mut host, "first");
    let reply = old
        .call(DataCall {
            context: context(),
            call_id: 1,
            request: DataOperation::SettingGet {
                key: "title".into(),
            },
        })
        .unwrap();
    let (cancel_tx, cancel_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let binding = host
            .open_request(
                context(),
                &definitions(),
                &BTreeMap::from([("title".into(), json!("second"))]),
                Instant::now() + Duration::from_secs(5),
                || {
                    cancel_tx.send(()).unwrap();
                    Ok(())
                },
            )
            .unwrap();
        done_tx.send(()).unwrap();
        (host, binding)
    });
    cancel_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(
        call(
            &old,
            2,
            DataOperation::StorageSet {
                key: "forbidden".into(),
                value: json!(true)
            }
        ),
        Err(DataRuntimeError::InvalidContext)
    );
    assert!(done_rx.recv_timeout(Duration::from_millis(30)).is_err());
    drop(reply);
    done_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let (_host, new) = worker.join().unwrap();
    assert_eq!(
        call(
            &new,
            1,
            DataOperation::SettingGet {
                key: "title".into()
            }
        ),
        Ok(json!("second"))
    );
    assert_eq!(
        call(
            &old,
            2,
            DataOperation::SettingGet {
                key: "title".into()
            }
        ),
        Err(DataRuntimeError::InvalidContext)
    );
}
#[test]
fn cancelled_transport_failure_and_drop_reject_retained_bindings() {
    let temp = Temp::new();
    let mut host = temp.host();
    let old = open(&mut host, "first");
    assert_eq!(
        host.close(|| Err(DataRuntimeError::Storage)),
        Err(DataRuntimeError::Storage)
    );
    assert!(host
        .open_request(
            context(),
            &definitions(),
            &BTreeMap::new(),
            Instant::now() + Duration::from_secs(1),
            || Ok(())
        )
        .is_err());
    assert_eq!(
        call(
            &old,
            1,
            DataOperation::SettingGet {
                key: "title".into()
            }
        ),
        Err(DataRuntimeError::InvalidContext)
    );
    drop(old);
    drop(host);
    let mut host = temp.host();
    let binding = open(&mut host, "next");
    drop(host);
    assert_eq!(
        call(
            &binding,
            1,
            DataOperation::StorageGet {
                key: "sample".into()
            }
        ),
        Err(DataRuntimeError::InvalidContext)
    );
}
#[test]
fn invalid_update_and_epoch_overflow_publish_no_new_request_or_old_authority() {
    let temp = Temp::new();
    let mut host = temp.host();
    let old = open(&mut host, "first");
    assert!(host
        .open_request(
            context(),
            &definitions(),
            &BTreeMap::from([("secret".into(), json!("never-persist"))]),
            Instant::now() + Duration::from_secs(2),
            || Ok(())
        )
        .is_err());
    assert!(host.active.is_none());
    assert_eq!(
        call(
            &old,
            1,
            DataOperation::SettingGet {
                key: "title".into()
            }
        ),
        Err(DataRuntimeError::InvalidContext)
    );
    let raw =
        fs::read_to_string(temp.0.join("settings/com.uipilot.fixture/settings.json")).unwrap();
    assert!(!raw.contains("never-persist"));
    assert!(raw.contains("first"));
    host.next_epoch = u64::MAX;
    assert!(host
        .open_request(
            context(),
            &definitions(),
            &BTreeMap::new(),
            Instant::now() + Duration::from_secs(2),
            || Ok(())
        )
        .is_err());
    assert!(host.active.is_none());
}
#[test]
fn typed_operations_reject_remote_configuration_and_keep_json_null_write() {
    let good = json!({"context":context(),"callId":1,"request":{"operation":"storageSet","key":"sample","value":null}});
    assert!(serde_json::from_value::<DataCall>(good).is_ok());
    for request in [
        json!({"operation":"settingSet","key":"title","value":"bad"}),
        json!({"operation":"settingGet","key":"title","definitions":[]}),
        json!({"operation":"storageGet","key":"sample","owner":"other"}),
        json!({"operation":"storageSet","key":"sample"}),
    ] {
        assert!(serde_json::from_value::<DataCall>(
            json!({"context":context(),"callId":1,"request":request})
        )
        .is_err());
    }
}

#[test]
fn deadline_spent_draining_does_not_start_a_settings_transaction() {
    let temp = Temp::new();
    let mut host = temp.host();
    let old = open(&mut host, "first");
    let path = temp.0.join("settings/com.uipilot.fixture/settings.json");
    let before = fs::read(&path).unwrap();
    let reply = old
        .call(DataCall {
            context: context(),
            call_id: 1,
            request: DataOperation::SettingGet {
                key: "title".into(),
            },
        })
        .unwrap();
    let (tx, rx) = mpsc::channel();
    let deadline = Instant::now() + Duration::from_millis(80);
    let worker = thread::spawn(move || {
        let result = host.open_request(
            context(),
            &definitions(),
            &BTreeMap::from([("title".into(), json!("must-not-save"))]),
            deadline,
            || {
                tx.send(()).unwrap();
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(host.active.is_none());
    });
    rx.recv_timeout(Duration::from_secs(1)).unwrap();
    if let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        thread::sleep(remaining + Duration::from_millis(5));
    }
    drop(reply);
    worker.join().unwrap();
    assert_eq!(fs::read(path).unwrap(), before);
}
