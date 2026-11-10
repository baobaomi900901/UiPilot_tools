//! Fixed installed-package process cases, with real lifecycle-owned retirement.
use super::*;
use crate::{
    lifecycle_macos::{InstalledRequest, LifecycleError, MacPluginLifecycle},
    manifest::{PublicPermission, PublicPlatform, PublicPluginHost},
    package_macos::read_directory,
};
const ID: &str = "com.uipilot.demo-settings";
const REQUEST: &str = "public-request-0000000000000001";
fn load(root: &Path) -> MacPluginLifecycle {
    MacPluginLifecycle::load(
        &root.join("inventory"),
        &root.join("storage"),
        &root.join("settings"),
    )
    .unwrap()
}
fn install(host: &mut MacPluginLifecycle, source: &Path) {
    let package =
        read_directory(source, &PublicPluginHost::current(PublicPlatform::Macos)).unwrap();
    let prepared = host.prepare_install(package).unwrap();
    let approved = prepared.required_permissions().clone();
    assert!(host.commit_install(prepared, &approved).unwrap());
}
fn attach(host: &mut MacPluginLifecycle, helper: &Path) -> (InstalledRequest, Arc<Mutex<Harness>>) {
    let mut transport = None;
    let request = host
        .open_request(
            ID,
            "default",
            REQUEST,
            Instant::now() + Duration::from_secs(5),
            |request| {
                let source = request
                    .package()
                    .resource(&request.package().manifest().runtime.entry)
                    .ok_or(LifecycleError::InvalidPackage)?;
                // Fixed diagnostic budget and one ESM entry. QuickJS has no module loader;
                // imports fail, rather than reading source directories or another package.
                if source.len() > 2048 {
                    return Err(LifecycleError::Capacity);
                }
                let source =
                    std::str::from_utf8(source).map_err(|_| LifecycleError::InvalidPackage)?;
                let harness = Arc::new(Mutex::new(
                    Harness::new_source(
                        helper,
                        "installedDemo",
                        Some(request.binding()),
                        Some(source),
                    )
                    .map_err(|_| LifecycleError::Runtime)?,
                ));
                transport = Some(harness.clone());
                Ok(Box::new(move || {
                    harness.lock().unwrap().stop();
                    Ok(())
                }))
            },
        )
        .unwrap();
    (request, transport.unwrap())
}
fn execute(host: &mut MacPluginLifecycle, helper: &Path, label: &str) -> Report {
    let (request, transport) = attach(host, helper);
    let mut calls = 0;
    let result = (|| -> Result<String, ()> {
        let mut harness = transport.lock().unwrap();
        loop {
            match harness.event()? {
                Event::Frame(Frame::DataCall { request: call }) => {
                    let id = call.call_id;
                    let reply = request.binding().call(call).map_err(|_| ())?;
                    calls += 1;
                    let body = match reply.result {
                        Ok(value) => json!({"ok":true,"value":value}),
                        Err(error) => json!({"ok":false,"error":error}),
                    };
                    let context = harness.context.clone();
                    harness.write(
                        json!({"kind":"reply","context":context,"callId":id,"value":body}),
                        Some(reply.permit),
                    )?;
                }
                Event::Frame(Frame::Complete { context, response }) => {
                    return harness.main_completion(request.binding(), context, response)
                }
                Event::StderrEof => harness.stderr_eof = true,
                _ => return Err(()),
            }
        }
    })();
    // Never hold the transport mutex while lifecycle invokes captured cleanup.
    host.close().unwrap();
    let harness = transport.lock().unwrap();
    let report = Report {
        pid: harness.pipe.pid(),
        accepted: result.is_ok(),
        backend_calls: calls,
        data: result.ok(),
        reaped: harness.reaped,
    };
    println!("BRIDGE_CASE {}", json!({"case":label,"report":report}));
    report
}
/// Test-root-only scenario. Source is removed after installation; execution and
/// restart must use the verified snapshot, not include_str! or the old path.
pub fn installed_scenario(helper: &Path, root: &Path) -> Vec<Report> {
    let mut host = load(root);
    install(&mut host, &root.join("package"));
    let manifest = std::fs::read(root.join("package/plugin.json")).unwrap();
    let source = std::fs::read_to_string(root.join("package/runtime.js")).unwrap();
    host.save_settings(
        ID,
        &BTreeMap::from([
            ("greeting".into(), json!("中文 hello")),
            ("count".into(), json!(3)),
            ("uppercase".into(), json!(true)),
            ("style".into(), json!("boxed")),
        ]),
    )
    .unwrap();
    std::fs::remove_file(root.join("package/plugin.json")).unwrap();
    std::fs::remove_file(root.join("package/runtime.js")).unwrap();
    let first = execute(&mut host, helper, "installedOriginal");
    drop(host);
    let mut host = load(root);
    let restored = execute(&mut host, helper, "installedRestored");
    let mut manifest: Value = serde_json::from_slice(&manifest).unwrap();
    manifest["version"] = json!("1.0.1");
    std::fs::write(
        root.join("package/plugin.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.join("package/runtime.js"),
        source.replace("const BUILD = '1.0.0'", "const BUILD = 'installed-upgrade'"),
    )
    .unwrap();
    install(&mut host, &root.join("package"));
    let upgraded = execute(&mut host, helper, "installedUpgraded");
    vec![first, restored, upgraded]
}
/// The real child is waiting for its first settings reply. Drop the unsent reply
/// permit before synchronous lifecycle mutation, then cancel/reap via retirement.
/// This proves transport cancellation, not an OS capability or clipboard effect.
pub fn installed_cancel_scenario(helper: &Path, root: &Path, revoke: bool) -> Vec<Report> {
    let mut host = load(root);
    install(&mut host, &root.join("package"));
    let (request, transport) = attach(&mut host, helper);
    let call = loop {
        let mut harness = transport.lock().unwrap();
        match harness.event().unwrap() {
            Event::Frame(Frame::DataCall { request }) => break request,
            Event::StderrEof => harness.stderr_eof = true,
            _ => panic!("expected fixed first settings call"),
        }
    };
    let reply = request.binding().call(call).unwrap();
    assert!(reply.result.is_ok());
    drop(reply);
    if revoke {
        assert!(host
            .set_permission(ID, PublicPermission::ClipboardWrite, false)
            .unwrap());
    } else {
        assert!(host.set_enabled(ID, false).unwrap());
    }
    assert!(request
        .binding()
        .session()
        .begin_call(request.binding().endpoint(), request.binding().context(), 2)
        .is_err());
    let report = {
        let harness = transport.lock().unwrap();
        Report {
            pid: harness.pipe.pid(),
            accepted: false,
            backend_calls: 1,
            data: None,
            reaped: harness.reaped,
        }
    };
    println!(
        "BRIDGE_CASE {}",
        json!({"case":if revoke{"installedRevoke"}else{"installedDisable"},"report":report})
    );
    // Requests retain data-service Arcs; release all old tickets before reopening
    // the same private root, never weaken flock to accommodate stale owners.
    drop(request);
    drop(transport);
    drop(host);
    let mut host = load(root);
    if revoke {
        assert!(host
            .set_permission(ID, PublicPermission::ClipboardWrite, true)
            .unwrap());
    } else {
        assert!(host
            .open_request(
                ID,
                "default",
                REQUEST,
                Instant::now() + Duration::from_secs(5),
                |_| panic!("disabled must not attach")
            )
            .is_err());
        assert!(host.set_enabled(ID, true).unwrap());
    }
    let fresh = execute(&mut host, helper, "installedAfterRetirement");
    vec![report, fresh]
}
