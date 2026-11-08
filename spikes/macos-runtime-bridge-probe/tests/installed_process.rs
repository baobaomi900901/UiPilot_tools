use serde_json::Value;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use uipilot_runtime_bridge_probe::host::{installed_cancel_scenario, installed_scenario};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let root = PathBuf::from(format!(
            "/private/tmp/uipilot-installed-process-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["inventory", "storage", "settings", "package"] {
            fs::create_dir(root.join(name)).unwrap();
            fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o700)).unwrap();
        }
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/public-plugins/com.uipilot.demo-settings/package");
        for name in ["plugin.json", "runtime.js"] {
            fs::copy(source.join(name), root.join("package").join(name)).unwrap();
        }
        Self(root)
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
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_uipilot-runtime-bridge-probe")))
}
#[test]
fn installed_original_module_survives_source_removal_restart_and_atomic_upgrade() {
    let temp = Temp::new();
    let reports = installed_scenario(&helper(), &temp.0);
    assert_eq!(reports.len(), 3);
    for (index, report) in reports.iter().enumerate() {
        assert!(report.accepted && report.reaped);
        assert_eq!(report.backend_calls, 4);
        let data: Value = serde_json::from_str(report.data.as_deref().unwrap()).unwrap();
        assert_eq!(data["results"][0]["title"], "【中文 HELLO × 3】");
        assert_eq!(
            data["results"][0]["subtitle"],
            if index < 2 {
                "uppercase=true · style=boxed · build 1.0.0"
            } else {
                "uppercase=true · style=boxed · build installed-upgrade"
            }
        );
    }
}
#[test]
fn disable_and_permission_revoke_cancel_real_waiting_transport_and_old_binding() {
    for revoke in [false, true] {
        let temp = Temp::new();
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(temp.0.join("package/plugin.json")).unwrap()).unwrap();
        manifest["permissions"] = serde_json::json!(["clipboard.write"]);
        fs::write(
            temp.0.join("package/plugin.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let reports = installed_cancel_scenario(&helper(), &temp.0, revoke);
        assert_eq!(reports.len(), 2);
        assert!(!reports[0].accepted && reports[0].reaped);
        assert_eq!(reports[0].backend_calls, 1);
        assert!(reports[1].accepted && reports[1].reaped);
        assert_eq!(reports[1].backend_calls, 4);
    }
}
