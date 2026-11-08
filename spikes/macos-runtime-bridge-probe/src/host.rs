//! Supervised fixed fixtures: testMemory plus actual private data services.
//! Internal transport, not the production plugin loader or an OS sandbox.
use crate::{
    call_session::{CallEndpoint, CallPermit, CallSession, ReplyPermit},
    data_call_gate::{PluginDataCallGate, PluginDataCallIdentity},
    request_contract::PluginRequestContext,
    runtime_data_macos::{DataCall, RuntimeDataBinding, RuntimeDataHost},
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Frame {
    DataCall {
        request: DataCall,
    },
    Call {
        context: PluginRequestContext,
        #[serde(rename = "callId")]
        call_id: u64,
        operation: String,
        key: String,
        value: String,
    },
    Complete {
        context: PluginRequestContext,
        response: Response,
    },
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Response {
    request_id: String,
    data: String,
}
enum Event {
    Frame(Frame),
    Eof,
    StderrEof,
    Error,
}
struct WriteJob {
    bytes: Vec<u8>,
    permit: Option<ReplyPermit>,
    ack: SyncSender<bool>,
}
struct OwnedChild(Child);
impl std::ops::Deref for OwnedChild {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.0
    }
}
impl std::ops::DerefMut for OwnedChild {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Harness {
    child: OwnedChild,
    writers: Option<SyncSender<WriteJob>>,
    events: Option<Receiver<Event>>,
    threads: Vec<JoinHandle<()>>,
    session: Arc<CallSession>,
    endpoint: CallEndpoint,
    context: PluginRequestContext,
    deadline: Instant,
    reaped: bool,
    stderr_eof: bool,
    sent: usize,
    managed_data: bool,
}
fn context() -> PluginRequestContext {
    PluginRequestContext {
        plugin_id: "com.uipilot.fixture".into(),
        plugin_generation: 1,
        request_id: "public-request-0000000000000001".into(),
    }
}
impl Harness {
    fn new(path: &Path, mode: &str) -> Result<Self, ()> {
        Self::new_bound(path, mode, None)
    }
    fn new_bound(
        path: &Path,
        mode: &str,
        binding: Option<&RuntimeDataBinding>,
    ) -> Result<Self, ()> {
        let context = binding.map(|b| b.context().clone()).unwrap_or_else(context);
        let deadline = Instant::now() + Duration::from_secs(2);
        let (session, endpoint) = match binding {
            Some(b) => (b.session().clone(), b.endpoint().clone()),
            None => {
                let gate = Arc::new(PluginDataCallGate::default());
                gate.activate(
                    PluginDataCallIdentity::new(&context.plugin_id, 1, 1, 1).map_err(|_| ())?,
                )
                .map_err(|_| ())?;
                let session = Arc::new(CallSession::new(gate));
                let endpoint = session
                    .open(context.clone(), 1, 1, deadline)
                    .map_err(|_| ())?
                    .0;
                (session, endpoint)
            }
        };
        let mut child = OwnedChild(
            Command::new(path)
                .env_clear()
                .env("LC_ALL", "C")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|_| ())?,
        );
        let mut input = child.stdin.take().ok_or(())?;
        let mut output = child.stdout.take().ok_or(())?;
        let mut error = child.stderr.take().ok_or(())?;
        let (tx, rx) = mpsc::sync_channel(2);
        let (writer_tx, writer_rx) = mpsc::sync_channel::<WriteJob>(1);
        let budget = Arc::new(AtomicUsize::new(0));
        let out_budget = budget.clone();
        let out_tx = tx.clone();
        let mut harness = Self {
            child,
            writers: Some(writer_tx),
            events: Some(rx),
            threads: Vec::new(),
            session,
            endpoint,
            context,
            deadline,
            reaped: false,
            stderr_eof: false,
            sent: 0,
            managed_data: binding.is_some(),
        };
        let reader = thread::Builder::new()
            .spawn(move || loop {
                let mut header = [0u8; 4];
                let first = match output.read(&mut header[..1]) {
                    Ok(0) => {
                        let _ = out_tx.send(Event::Eof);
                        break;
                    }
                    Ok(_) => 1,
                    Err(_) => 0,
                };
                if first == 0 || output.read_exact(&mut header[1..]).is_err() {
                    let _ = out_tx.send(Event::Error);
                    break;
                }
                let len = u32::from_be_bytes(header) as usize;
                if len > 4096 || out_budget.fetch_add(4 + len, Ordering::SeqCst) + 4 + len > 8192 {
                    let _ = out_tx.send(Event::Error);
                    break;
                }
                let mut bytes = vec![0u8; len];
                if output.read_exact(&mut bytes).is_err() {
                    let _ = out_tx.send(Event::Error);
                    break;
                }
                if crate::storage_document::validate_mac_json(&bytes).is_none() {
                    let _ = out_tx.send(Event::Error);
                    break;
                }
                match serde_json::from_slice(&bytes) {
                    Ok(frame) => {
                        if out_tx.send(Event::Frame(frame)).is_err() {
                            break;
                        }
                    }
                    Err(_) => {
                        let _ = out_tx.send(Event::Error);
                        break;
                    }
                }
            })
            .map_err(|_| ())?;
        harness.threads.push(reader);
        let stderr = thread::Builder::new()
            .spawn(move || {
                let mut bytes = [0u8; 512];
                loop {
                    match error.read(&mut bytes) {
                        Ok(0) => break,
                        Ok(n) => {
                            if budget.fetch_add(n, Ordering::SeqCst) + n > 8192 {
                                let _ = tx.send(Event::Error);
                                break;
                            }
                        }
                        Err(_) => {
                            let _ = tx.send(Event::Error);
                            break;
                        }
                    }
                }
                let _ = tx.send(Event::StderrEof);
            })
            .map_err(|_| ())?;
        harness.threads.push(stderr);
        let writer = thread::Builder::new()
            .spawn(move || {
                while let Ok(job) = writer_rx.recv() {
                    let okay = input
                        .write_all(&job.bytes)
                        .and_then(|_| input.flush())
                        .is_ok();
                    // Drop the reply permit before ack; the single reader loop never
                    // processes completion while an acknowledged write is still counted.
                    drop(job.permit);
                    let _ = job.ack.send(okay);
                    if !okay {
                        break;
                    }
                }
            })
            .map_err(|_| ())?;
        harness.threads.push(writer);
        harness.write(json!({"mode":mode,"context":harness.context}), None)?;
        Ok(harness)
    }
    fn remaining(&self) -> Result<Duration, ()> {
        self.deadline
            .checked_duration_since(Instant::now())
            .ok_or(())
    }
    fn write(&mut self, value: Value, permit: Option<ReplyPermit>) -> Result<(), ()> {
        let body = serde_json::to_vec(&value).map_err(|_| ())?;
        if body.len() > 4096 {
            return Err(());
        }
        self.sent = self.sent.checked_add(4 + body.len()).ok_or(())?;
        if self.sent > 8192 {
            return Err(());
        }
        let mut bytes = (body.len() as u32).to_be_bytes().to_vec();
        bytes.extend(body);
        let (tx, rx) = mpsc::sync_channel(1);
        self.writers
            .as_ref()
            .ok_or(())?
            .try_send(WriteJob {
                bytes,
                permit,
                ack: tx,
            })
            .map_err(|_| ())?;
        if rx.recv_timeout(self.remaining()?).map_err(|_| ())? {
            Ok(())
        } else {
            Err(())
        }
    }
    fn event(&self) -> Result<Event, ()> {
        self.events
            .as_ref()
            .ok_or(())?
            .recv_timeout(self.remaining()?)
            .map_err(|_| ())
    }
    fn call(&self, frame: Frame) -> Result<(CallPermit, u64, String, String, String), ()> {
        let Frame::Call {
            context,
            call_id,
            operation,
            key,
            value,
        } = frame
        else {
            return Err(());
        };
        if key != "sample"
            || value.len() > 1024
            || !matches!(operation.as_str(), "testMemory.read" | "testMemory.write")
        {
            return Err(());
        }
        let permit = self
            .session
            .begin_call(&self.endpoint, &context, call_id)
            .map_err(|_| ())?;
        Ok((permit, call_id, operation, key, value))
    }
    fn reply(&mut self, call: CallPermit, id: u64, value: Value) -> Result<(), ()> {
        let permit = self
            .session
            .begin_reply(&self.endpoint, call)
            .map_err(|_| ())?;
        self.write(
            json!({"kind":"reply","context":self.context,"callId":id,"value":value}),
            Some(permit),
        )
    }
    fn completion(
        &mut self,
        context: PluginRequestContext,
        response: Response,
    ) -> Result<String, ()> {
        let ticket = self
            .session
            .prepare_completion(&self.endpoint, &context, &response.request_id)
            .map_err(|_| ())?;
        // Completion cannot be followed by another frame or an unsuccessful exit.
        let mut stdout_eof = false;
        while !stdout_eof || !self.stderr_eof {
            match self.event()? {
                Event::Eof if !stdout_eof => stdout_eof = true,
                Event::StderrEof if !self.stderr_eof => self.stderr_eof = true,
                _ => return Err(()),
            }
        }
        loop {
            if let Some(status) = self.child.try_wait().map_err(|_| ())? {
                if !status.success() {
                    return Err(());
                }
                break;
            }
            self.remaining()?;
            thread::sleep(Duration::from_millis(2));
        }
        self.session
            .submit_completion(&self.endpoint, ticket, || response.data)
            .map_err(|_| ())
    }
    fn stop(&mut self) {
        if self.reaped {
            return;
        }
        // Data controller owns gate/session retirement and calls stop as its
        // cancellation hook. Legacy memory fixture owns its session here.
        let drain = if self.managed_data {
            None
        } else {
            self.session.close(&self.endpoint).ok()
        };
        let _ = self.child.kill();
        self.reaped = self.child.wait().is_ok();
        drop(self.writers.take());
        drop(self.events.take());
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
        if let Some(drain) = drain {
            let _ = drain.wait();
        }
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.stop();
    }
}
#[derive(Debug, serde::Serialize)]
pub struct Report {
    pub pid: u32,
    pub accepted: bool,
    pub backend_calls: usize,
    pub data: Option<String>,
    pub reaped: bool,
}

pub fn scenario(path: &Path, mode: &str) -> Report {
    let mut host = Harness::new(path, mode).expect("fixed helper spawn");
    if matches!(mode, "late" | "partialHeader" | "partialBody") {
        host.deadline = Instant::now() + Duration::from_millis(150);
    }
    let mut storage = BTreeMap::new();
    let mut calls = 0;
    let result = (|| -> Result<String, ()> {
        loop {
            match host.event()? {
                Event::Frame(Frame::Complete { context, response }) => {
                    return host.completion(context, response)
                }
                Event::Frame(frame) => {
                    let (permit, id, operation, key, value) = host.call(frame)?;
                    calls += 1;
                    let answer = if operation == "testMemory.write" {
                        storage.insert(key, value);
                        Value::Null
                    } else {
                        storage.get(&key).map_or(Value::Null, |value| json!(value))
                    };
                    host.reply(permit, id, answer)?;
                }
                Event::StderrEof => host.stderr_eof = true,
                Event::Error | Event::Eof => return Err(()),
            }
        }
    })();
    host.stop();
    let report = Report {
        pid: host.child.id(),
        accepted: result.is_ok(),
        backend_calls: calls,
        data: result.ok(),
        reaped: host.reaped,
    };
    println!("BRIDGE_CASE {}", json!({"case":mode,"report":report}));
    report
}

/// Actual disk services + captured request authority + real QuickJS calls.
/// The caller supplies a newly-created private TEST root, never user data.
pub fn data_scenario(path: &Path, mode: &str, root: &Path, update: bool) -> Report {
    use crate::manifest::{parse_manifest, PublicPlatform, PublicPluginHost};
    let manifest = parse_manifest(
        include_bytes!(
            "../../../examples/public-plugins/com.uipilot.demo-settings/package/plugin.json"
        ),
        &PublicPluginHost {
            platform: PublicPlatform::Macos,
            version: [0, 19, 0],
            api_version: 1,
        },
    )
    .unwrap();
    let mut data_host =
        RuntimeDataHost::load(&root.join("storage"), &root.join("settings")).unwrap();
    let mut context = context();
    context.plugin_id = manifest.plugin_id;
    let updates = if update {
        BTreeMap::from([
            ("greeting".into(), json!("中文 hello")),
            ("count".into(), json!(3)),
            ("uppercase".into(), json!(true)),
            ("style".into(), json!("boxed")),
        ])
    } else {
        BTreeMap::new()
    };
    let binding = data_host
        .open_request(
            context,
            &manifest.settings,
            &updates,
            Instant::now() + Duration::from_secs(5),
            || Ok(()),
        )
        .unwrap();
    let mut host = Harness::new_bound(path, mode, Some(&binding)).unwrap();
    let mut calls = 0;
    let result = (|| -> Result<String, ()> {
        loop {
            match host.event()? {
                Event::Frame(Frame::Complete { context, response }) => {
                    // A child-controlled outer envelope cannot repair or hide
                    // the plugin result's own request identity.
                    crate::storage_document::validate_mac_json(response.data.as_bytes())
                        .ok_or(())?;
                    let result: Value = serde_json::from_str(&response.data).map_err(|_| ())?;
                    if result.get("requestId").and_then(Value::as_str)
                        != Some(host.context.request_id.as_str())
                    {
                        return Err(());
                    }
                    return host.completion(context, response);
                }
                Event::Frame(Frame::DataCall { request }) => {
                    let id = request.call_id;
                    let reply = binding.call(request).map_err(|_| ())?;
                    calls += 1;
                    if mode == "dataRevoke" {
                        data_host
                            .close(|| {
                                host.stop();
                                drop(reply);
                                Ok(())
                            })
                            .map_err(|_| ())?;
                        return Err(());
                    }
                    let body = match reply.result {
                        Ok(value) => json!({"ok":true,"value":value}),
                        Err(error) => json!({"ok":false,"error":error}),
                    };
                    host.write(
                        json!({"kind":"reply","context":host.context,"callId":id,"value":body}),
                        Some(reply.permit),
                    )?;
                }
                Event::StderrEof => host.stderr_eof = true,
                _ => return Err(()),
            }
        }
    })();
    data_host
        .close(|| {
            host.stop();
            Ok(())
        })
        .unwrap();
    let report = Report {
        pid: host.child.id(),
        accepted: result.is_ok(),
        backend_calls: calls,
        data: result.ok(),
        reaped: host.reaped,
    };
    println!("BRIDGE_CASE {}", json!({"case":mode,"report":report}));
    report
}

/// Real child blocked waiting for a real admitted backend call. Close is proven
/// linearized before checking ack; admitted mutation may finish before drain ack.
pub fn revoke_blocked(path: &Path) -> Report {
    let host = Harness::new(path, "roundtrip").unwrap();
    let pid = host.child.id();
    let Event::Frame(frame) = host.event().unwrap() else {
        panic!("expected call")
    };
    let (permit, _, _, _, _) = host.call(frame).unwrap();
    let session = host.session.clone();
    let endpoint = host.endpoint.clone();
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let effects = Arc::new(AtomicUsize::new(0));
    let worker_effects = effects.clone();
    let backend = thread::spawn(move || {
        started_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        worker_effects.fetch_add(1, Ordering::SeqCst);
        drop(permit);
    });
    started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    // Retire synchronously, proving new admission is closed before waiting.
    let drain = session.close(&endpoint).unwrap();
    assert!(session.begin_call(&endpoint, &context(), 2).is_err());
    let (tx, rx) = mpsc::sync_channel(1);
    let closer = thread::spawn(move || {
        let mut host = host;
        host.stop();
        drain.wait().unwrap();
        tx.send(host.reaped).unwrap();
    });
    assert!(rx.recv_timeout(Duration::from_millis(25)).is_err());
    release_tx.send(()).unwrap();
    backend.join().unwrap();
    let reaped = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    closer.join().unwrap();
    let report = Report {
        pid,
        accepted: false,
        backend_calls: effects.load(Ordering::SeqCst),
        data: None,
        reaped,
    };
    println!(
        "BRIDGE_CASE {}",
        json!({"case":"revokeBlocked","report":report})
    );
    report
}

#[cfg(test)]
mod wire_tests {
    use super::*;
    #[test]
    fn actual_typed_frame_rejects_duplicates_and_unknown_fields() {
        let context = serde_json::to_string(&context()).unwrap();
        let base = format!(
            r#"{{"kind":"call","context":{context},"callId":1,"operation":"testMemory.read","key":"sample","value":""}}"#
        );
        assert!(serde_json::from_str::<Frame>(&base).is_ok());
        for wire in [
            base.replace("\"kind\":\"call\"", "\"kind\":\"call\",\"kind\":\"call\""),
            base.replace("\"callId\":1", "\"callId\":1,\"callId\":1"),
            base.replace(
                &format!("\"context\":{context}"),
                &format!("\"context\":{context},\"context\":{context}"),
            ),
            base.replace("\"key\":", "\"extra\":true,\"key\":"),
            format!(
                r#"{{"kind":"complete","context":{context},"response":{{"requestId":"one","requestId":"two","data":"x"}}}}"#
            ),
        ] {
            assert!(serde_json::from_str::<Frame>(&wire).is_err(), "{wire}");
        }
    }
}
