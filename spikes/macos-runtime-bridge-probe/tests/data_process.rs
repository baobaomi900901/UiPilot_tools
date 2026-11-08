use serde_json::{json, Value};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use uipilot_runtime_bridge_probe::host::data_scenario;
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = PathBuf::from(format!(
            "/private/tmp/uipilot-data-process-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&p).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["storage", "settings"] {
            fs::create_dir(p.join(name)).unwrap();
            fs::set_permissions(p.join(name), fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn helper() -> PathBuf {
    std::env::var_os("UIPILOT_BRIDGE_HELPER")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_BIN_EXE_uipilot-runtime-bridge-probe")).into())
}
#[test]
fn original_demo_settings_reads_real_saved_values_after_host_and_child_restart() {
    let temp = Temp::new();
    for update in [true, false] {
        let report = data_scenario(&helper(), "dataDemo", &temp.0, update);
        assert!(report.accepted && report.reaped);
        assert_eq!(report.backend_calls, 4);
        let result: Value = serde_json::from_str(report.data.as_deref().unwrap()).unwrap();
        assert_eq!(result["results"][0]["title"], json!("【中文 HELLO × 3】"));
        assert_eq!(
            result["results"][0]["subtitle"],
            json!("uppercase=true · style=boxed · build 1.0.0")
        );
    }
}
#[test]
fn actual_json_storage_survives_process_reopen_and_missing_setting_rejects() {
    let temp = Temp::new();
    for (mode, count) in [("dataStorageWrite", 2), ("dataStorageRead", 1)] {
        let report = data_scenario(&helper(), mode, &temp.0, false);
        assert!(report.accepted && report.reaped);
        assert_eq!(report.backend_calls, count);
        let result: Value = serde_json::from_str(report.data.as_deref().unwrap()).unwrap();
        assert_eq!(
            result["data"],
            json!({"text":"中文 😀","values":[false,0,null]})
        );
    }
    let report = data_scenario(&helper(), "dataDenied", &temp.0, false);
    assert!(report.accepted && report.reaped);
    assert_eq!(report.backend_calls, 1);
    let result: Value = serde_json::from_str(report.data.as_deref().unwrap()).unwrap();
    assert_eq!(result["data"], json!("invalidOperation"));
}
#[test]
fn actual_child_cannot_choose_owner_set_settings_or_replay_a_call() {
    let temp = Temp::new();
    for (mode, calls) in [("dataForged", 0), ("dataSetter", 0), ("dataDuplicate", 1)] {
        let report = data_scenario(&helper(), mode, &temp.0, false);
        assert!(!report.accepted && report.reaped, "{mode}");
        assert_eq!(report.backend_calls, calls);
        assert!(!temp.0.join("storage/com.uipilot.other").exists());
        let fresh = data_scenario(&helper(), "dataDemo", &temp.0, false);
        assert!(fresh.accepted && fresh.reaped);
    }
}
#[test]
fn child_result_identity_is_not_repaired_by_outer_envelope() {
    let temp = Temp::new();
    for mode in ["dataWrongResult", "dataMissingResult", "dataWrongEnvelope"] {
        let report = data_scenario(&helper(), mode, &temp.0, false);
        assert!(!report.accepted && report.reaped, "{mode}");
        assert_eq!(report.backend_calls, 0);
    }
}
#[test]
fn revoke_cancels_waiting_child_before_reply_drain_and_committed_write_is_not_rollback() {
    let temp = Temp::new();
    let report = data_scenario(&helper(), "dataRevoke", &temp.0, false);
    assert!(!report.accepted && report.reaped);
    assert_eq!(report.backend_calls, 1);
    let fresh = data_scenario(&helper(), "dataStorageRead", &temp.0, false);
    assert!(fresh.accepted && fresh.reaped);
    let result: Value = serde_json::from_str(fresh.data.as_deref().unwrap()).unwrap();
    assert_eq!(
        result["data"],
        json!({"text":"中文 😀","values":[false,0,null]})
    );
}

#[test]
fn real_main_result_contract_rejects_malformed_rows_and_unavailable_actions() {
    let temp = Temp::new();
    for mode in [
        "dataMainUnknown",
        "dataMainDuplicate",
        "dataMainTooMany",
        "dataMainCopy",
    ] {
        let report = data_scenario(&helper(), mode, &temp.0, false);
        assert!(!report.accepted && report.reaped, "{mode}");
        assert_eq!(report.backend_calls, 0);
    }
    let empty = data_scenario(&helper(), "dataMainEmpty", &temp.0, false);
    assert!(empty.accepted && empty.reaped);
    let result: Value = serde_json::from_str(empty.data.as_deref().unwrap()).unwrap();
    assert_eq!(result["results"], json!([]));
    let fresh = data_scenario(&helper(), "dataDemo", &temp.0, false);
    assert!(fresh.accepted && fresh.reaped);
}
