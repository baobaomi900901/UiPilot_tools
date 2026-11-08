use std::path::Path;
use uipilot_runtime_bridge_probe::host::{revoke_blocked, scenario};
fn helper() -> std::path::PathBuf {
    std::env::var_os("UIPILOT_BRIDGE_HELPER")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_BIN_EXE_uipilot-runtime-bridge-probe")).into())
}
#[test]
fn actual_quickjs_memory_roundtrip_then_single_completion() {
    let report = scenario(&helper(), "roundtrip");
    assert!(report.accepted && report.reaped);
    assert_eq!(report.backend_calls, 2);
    assert_eq!(report.data.as_deref(), Some("fixed"));
}
#[test]
fn wrong_owner_duplicate_call_early_exit_and_late_completion_fail_without_new_identity() {
    for (mode, expected) in [("forged", 0), ("duplicate", 1), ("exit", 0), ("late", 0)] {
        let report = scenario(&helper(), mode);
        assert!(!report.accepted && report.reaped, "{mode}");
        assert_eq!(report.backend_calls, expected);
        let fresh = scenario(&helper(), "roundtrip");
        assert!(fresh.accepted && fresh.reaped);
    }
}
#[test]
fn close_kills_child_before_waiting_for_admitted_backend_then_ack_drains() {
    let report = revoke_blocked(&helper());
    assert!(!report.accepted && report.reaped);
    assert_eq!(report.backend_calls, 1);
    assert!(scenario(&helper(), "roundtrip").accepted);
}

#[test]
fn stdout_eof_before_late_stderr_overflow_cannot_publish() {
    let report = scenario(&helper(), "stderrTail");
    assert!(!report.accepted && report.reaped);
    assert_eq!(report.backend_calls, 0);
}

#[test]
fn partial_oversized_duplicate_completion_and_trailing_bytes_are_reaped() {
    for mode in [
        "partialHeader",
        "partialBody",
        "oversizedFrame",
        "doubleComplete",
        "tailBytes",
    ] {
        let report = scenario(&helper(), mode);
        assert!(!report.accepted && report.reaped, "{mode}");
        assert_eq!(report.backend_calls, 0);
        let fresh = scenario(&helper(), "roundtrip");
        assert!(fresh.accepted && fresh.reaped);
    }
}
