use super::super::{
    request_contract::PluginRequestContext,
    runtime_data_macos::{DataCall, DataOperation, RuntimeDataHost},
};
use super::*;
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
pub(crate) struct Fixture {
    pub(crate) host: RuntimeDataHost,
    pub(crate) binding: RuntimeDataBinding,
    path: PathBuf,
}
impl Fixture {
    pub(crate) fn new() -> Self {
        Self::with_deadline(Instant::now() + Duration::from_secs(5))
    }
    fn with_deadline(deadline: Instant) -> Self {
        let path = PathBuf::from(format!(
            "/private/tmp/uipilot-main-result-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["storage", "settings"] {
            fs::create_dir(path.join(name)).unwrap();
            fs::set_permissions(path.join(name), fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut host =
            RuntimeDataHost::load(&path.join("storage"), &path.join("settings")).unwrap();
        let binding = host
            .open_request(context(), &[], &BTreeMap::new(), deadline, || Ok(()))
            .unwrap();
        Self {
            host,
            binding,
            path,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.host.close(|| Ok(()));
        let _ = fs::remove_dir_all(&self.path);
    }
}
pub(crate) fn context() -> PluginRequestContext {
    PluginRequestContext {
        plugin_id: "com.uipilot.fixture".into(),
        plugin_generation: 1,
        request_id: "public-request-0000000000000001".into(),
    }
}
pub(crate) fn raw() -> Vec<u8> {
    serde_json::to_vec(&json!({"requestId":context().request_id,"results":[{"id":"plugin-private-row-id","title":"中文 😀","subtitle":"subtitle","detail":"detail"}]})).unwrap()
}

#[test]
fn valid_result_is_typed_and_completion_is_single_use() {
    let f = Fixture::new();
    let ticket = f.binding.prepare_main_results(&raw()).unwrap();
    assert!(f.binding.prepare_main_results(&raw()).is_err());
    let rows = f.binding.submit_main_results(ticket, |rows| rows).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "中文 😀");
    assert_eq!(rows[0].copy_text, None);
    assert!(f.binding.prepare_main_results(&raw()).is_err());
}
#[test]
fn foreign_endpoint_equal_wire_and_closed_gate_cannot_submit() {
    let mut a = Fixture::new();
    let b = Fixture::new();
    let ticket = a.binding.prepare_main_results(&raw()).unwrap();
    assert_eq!(
        b.binding
            .submit_main_results(ticket, |_| panic!("foreign publication")),
        Err(MainCompletionError::Expired)
    );
    let ticket = b.binding.prepare_main_results(&raw()).unwrap();
    let old = b.binding.clone();
    drop(b);
    assert_eq!(
        old.submit_main_results(ticket, |_| panic!("revoked publication")),
        Err(MainCompletionError::Expired)
    );
    a.host.close(|| Ok(())).unwrap();
    assert!(a.binding.prepare_main_results(&raw()).is_err());
}
#[test]
fn pending_reply_blocks_completion_until_actual_permit_release() {
    let f = Fixture::new();
    let reply = f
        .binding
        .call(DataCall {
            context: context(),
            call_id: 1,
            request: DataOperation::StorageGet {
                key: "missing".into(),
            },
        })
        .unwrap();
    assert!(matches!(
        f.binding.prepare_main_results(&raw()),
        Err(MainCompletionError::Busy)
    ));
    drop(reply);
    let ticket = f.binding.prepare_main_results(&raw()).unwrap();
    assert_eq!(
        f.binding.submit_main_results(ticket, |rows| rows.len()),
        Ok(1)
    );
}
#[test]
fn raw_duplicate_oversize_and_unavailable_action_do_not_publish_or_repair() {
    let f = Fixture::new();
    let request = context().request_id;
    for bytes in [
        format!(r#"{{"requestId":"{request}","results":[],"results":[]}}"#).into_bytes(),
        format!(r#"{{"requestId":"{request}","results":[{{"id":"a","title":"a","title":"b"}}]}}"#)
            .into_bytes(),
        vec![b' '; MAX_MAIN_RESPONSE_BYTES + 1],
        serde_json::to_vec(&json!({"requestId":"wrong","results":[]})).unwrap(),
    ] {
        assert!(matches!(
            f.binding.prepare_main_results(&bytes),
            Err(MainCompletionError::InvalidResult)
        ));
    }
    let action=serde_json::to_vec(&json!({"requestId":request,"results":[{"id":"a","title":"a","defaultAction":{"type":"copyText","text":"must-not-copy"}}]})).unwrap();
    assert!(matches!(
        f.binding.prepare_main_results(&action),
        Err(MainCompletionError::UnsupportedAction)
    ));
    assert!(f.binding.prepare_main_results(&raw()).is_ok());
}
#[test]
fn deadline_after_prepare_prevents_sink_execution() {
    let deadline = Instant::now() + Duration::from_millis(150);
    let f = Fixture::with_deadline(deadline);
    let ticket = f.binding.prepare_main_results(&raw()).unwrap();
    if let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        std::thread::sleep(remaining + Duration::from_millis(5));
    }
    assert_eq!(
        f.binding
            .submit_main_results(ticket, |_| panic!("late publication")),
        Err(MainCompletionError::Expired)
    );
}
