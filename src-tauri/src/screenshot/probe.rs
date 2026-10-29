//! A dedicated, bounded Provider client. No pipe or UIA work runs on the overlay thread.
use super::model::{PhysicalPoint, PhysicalRect};
use serde_json::json;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    os::windows::{io::AsRawHandle, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::Manager;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::{ClientOptions, NamedPipeClient},
};

const MAX_RESPONSE: usize = 64 * 1024;
const HARD_TIMEOUT: Duration = Duration::from_millis(1000);
const MIN_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProbeReply {
    pub sequence: u64,
    pub point: PhysicalPoint,
    pub rect: Option<PhysicalRect>,
    pub unavailable: bool,
    pub target: Option<ProbeTarget>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProbeTarget {
    pub hwnd: isize,
    pub pid: u32,
    pub rect: PhysicalRect,
}
pub(crate) fn snapshot_target(hwnd: isize) -> Option<ProbeTarget> {
    use windows::Win32::{
        Foundation::{HWND, RECT},
        UI::WindowsAndMessaging::*,
    };
    let native = HWND(hwnd as *mut _);
    let mut pid = 0;
    let mut rect = RECT::default();
    unsafe {
        if !IsWindow(Some(native)).as_bool()
            || !IsWindowVisible(native).as_bool()
            || IsIconic(native).as_bool()
        {
            return None;
        }
        GetWindowThreadProcessId(native, Some(&mut pid));
        GetWindowRect(native, &mut rect).ok()?;
    }
    let rect = PhysicalRect {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    };
    if pid == 0 || rect.dimensions().is_err() {
        return None;
    }
    Some(ProbeTarget { hwnd, pid, rect })
}
pub(crate) fn target_is_current(target: ProbeTarget) -> bool {
    snapshot_target(target.hwnd) == Some(target)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Request {
    sequence: u64,
    point: PhysicalPoint,
}
#[derive(Default)]
struct Mailbox {
    sequence: u64,
    pending: Option<Request>,
    reply: Option<ProbeReply>,
    failed: bool,
}
impl Mailbox {
    fn request(&mut self, point: PhysicalPoint) -> u64 {
        self.sequence = self
            .sequence
            .checked_add(1)
            .expect("probe sequence exhausted");
        let request = Request {
            sequence: self.sequence,
            point,
        };
        if self.failed {
            self.reply = Some(ProbeReply {
                sequence: request.sequence,
                point,
                rect: None,
                unavailable: true,
                target: None,
            });
        } else {
            self.pending = Some(request);
        }
        self.sequence
    }
    fn fail(&mut self) {
        self.failed = true;
        let request = self.pending.take();
        self.reply = Some(ProbeReply {
            sequence: request.map_or(self.sequence, |r| r.sequence),
            point: request.map_or(PhysicalPoint::default(), |r| r.point),
            rect: None,
            unavailable: true,
            target: None,
        });
    }
}
#[derive(Default)]
struct Shared {
    mailbox: Mutex<Mailbox>,
    changed: Condvar,
    cancelled: AtomicBool,
}

pub(crate) struct ProbeWorker {
    shared: Arc<Shared>,
}
impl ProbeWorker {
    pub fn start(exe: PathBuf, excluded: Vec<isize>) -> Self {
        let shared = Arc::new(Shared::default());
        let worker = shared.clone();
        if std::thread::Builder::new()
            .name("screenshot-probe".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_worker(exe, excluded, &worker)
                }));
                if !worker.cancelled.load(Ordering::Acquire) && !matches!(result, Ok(Ok(()))) {
                    if let Ok(mut mailbox) = worker.mailbox.lock() {
                        mailbox.fail();
                    }
                }
            })
            .is_err()
        {
            shared.mailbox.lock().unwrap().fail();
        }
        Self { shared }
    }
    pub fn request(&self, point: PhysicalPoint) -> u64 {
        let sequence = self.shared.mailbox.lock().unwrap().request(point);
        self.shared.changed.notify_one();
        sequence
    }
    pub fn poll(&self) -> Option<ProbeReply> {
        self.shared.mailbox.lock().ok()?.reply.take()
    }
}
impl Drop for ProbeWorker {
    fn drop(&mut self) {
        self.shared.cancelled.store(true, Ordering::Release);
        self.shared.changed.notify_all();
    }
}

pub(crate) fn executable(app: &tauri::AppHandle) -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("UIPILOT_SCREENSHOT_PROVIDER_EXE") {
        let path = PathBuf::from(path);
        return path.is_absolute().then_some(path);
    }
    app.path()
        .resource_dir()
        .ok()
        .map(|root| root.join("resources/provider/UIAutoma.UIAutomation.Provider.exe"))
}

// A matching manifest makes a partially staged or accidentally stale executable
// unavailable, rather than silently running a different build than the UI reports.
fn verify_asset(exe: &Path) -> Result<(), ()> {
    let manifest_path = exe.parent().ok_or(())?.join("asset-manifest.json");
    let file = std::fs::File::open(manifest_path).map_err(|_| ())?;
    let mut bytes = Vec::new();
    file.take((MAX_RESPONSE + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() > MAX_RESPONSE {
        return Err(());
    }
    let manifest: Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
    if manifest["schema_version"] != 1 || manifest["capabilities"]["point_probe_v1"] != true {
        return Err(());
    }
    let mut file = std::fs::File::open(exe).map_err(|_| ())?;
    if Some(file.metadata().map_err(|_| ())?.len()) != manifest["artifact"]["size"].as_u64() {
        return Err(());
    }
    let expected = manifest["artifact"]["sha256"].as_str().ok_or(())?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer).map_err(|_| ())?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    if !format!("{:x}", hash.finalize()).eq_ignore_ascii_case(expected) {
        return Err(());
    }
    Ok(())
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run_worker(exe: PathBuf, excluded: Vec<isize>, shared: &Shared) -> Result<(), ()> {
    #[cfg(test)]
    eprintln!("probe: validate asset");
    verify_asset(&exe)?;
    if shared.cancelled.load(Ordering::Acquire) {
        return Ok(());
    }
    let nonce = unsafe { windows::Win32::System::Com::CoCreateGuid() }.map_err(|_| ())?;
    let pipe_name = format!(r"\\.\pipe\UiPilot.Screenshot.{nonce:?}");
    let mut child = OwnedChild(
        Command::new(&exe)
            .args([
                "--pipe-name",
                &pipe_name,
                "--parent-pid",
                &std::process::id().to_string(),
            ])
            .current_dir(exe.parent().ok_or(())?)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000)
            .spawn()
            .map_err(|_| ())?,
    );
    #[cfg(test)]
    eprintln!("probe: child spawned");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| ())?;
    let mut pipe = runtime.block_on(bounded(shared, async {
        loop {
            if child.0.try_wait().map_err(|_| ())?.is_some() {
                return Err(());
            }
            match ClientOptions::new().open(&pipe_name) {
                Ok(pipe) => {
                    let mut pid = 0;
                    unsafe {
                        windows::Win32::System::Pipes::GetNamedPipeServerProcessId(
                            windows::Win32::Foundation::HANDLE(pipe.as_raw_handle()),
                            &mut pid,
                        )
                    }
                    .map_err(|_| ())?;
                    if pid != child.0.id() {
                        return Err(());
                    }
                    return Ok(pipe);
                }
                Err(error) if matches!(error.raw_os_error(), Some(2 | 231)) => {
                    tokio::time::sleep(Duration::from_millis(10)).await
                }
                Err(_) => return Err(()),
            }
        }
    }))?;
    #[cfg(test)]
    eprintln!("probe: connected");
    let ping = runtime.block_on(bounded(
        shared,
        exchange(
            &mut pipe,
            json!({"jsonrpc":"2.0","id":0,"method":"system.ping","params":{}}),
        ),
    ))?;
    #[cfg(test)]
    eprintln!(
        "probe: ping capability={}",
        ping["result"]["capabilities"]["point_probe_v1"]
    );
    if ping["jsonrpc"] != "2.0"
        || ping["id"] != 0
        || ping.get("error").is_some()
        || ping["result"]["capabilities"]["point_probe_v1"] != true
    {
        return Err(());
    }
    let mut last = Instant::now()
        .checked_sub(MIN_INTERVAL)
        .unwrap_or_else(Instant::now);
    loop {
        let request = {
            let mut mailbox = shared.mailbox.lock().map_err(|_| ())?;
            loop {
                if shared.cancelled.load(Ordering::Acquire) {
                    return Ok(());
                }
                let remaining = MIN_INTERVAL.saturating_sub(last.elapsed());
                if remaining.is_zero() && mailbox.pending.is_some() {
                    break mailbox.pending.take().unwrap();
                }
                let wait = if mailbox.pending.is_some() {
                    remaining
                } else {
                    Duration::from_millis(100)
                };
                mailbox = shared
                    .changed
                    .wait_timeout(mailbox, wait)
                    .map_err(|_| ())?
                    .0;
            }
        };
        last = Instant::now();
        let response = runtime.block_on(bounded(shared,exchange(&mut pipe,json!({
            "jsonrpc":"2.0","id":request.sequence,"method":"probe.point","params":{
                "x":request.point.x,"y":request.point.y,
                "exclude_process_ids":[std::process::id(),child.0.id()],
                "exclude_hwnds":excluded.iter().map(|v|v.to_string()).collect::<Vec<_>>(),"timeout_ms":150
            }
        }))))?;
        #[cfg(test)]
        eprintln!("probe: response status={}", response["result"]["status"]);
        let mut rect = decode_reply(response.clone(), request.sequence, request.point)?;
        let target = if rect.is_some() {
            response["result"]["root_hwnd"]
                .as_str()
                .and_then(|v| v.parse().ok())
                .and_then(snapshot_target)
                .filter(|t| Some(t.pid as u64) == response["result"]["process_id"].as_u64())
        } else {
            None
        };
        if target.is_none() {
            rect = None;
        }
        let mut mailbox = shared.mailbox.lock().map_err(|_| ())?;
        if shared.cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        mailbox.reply = Some(ProbeReply {
            sequence: request.sequence,
            point: request.point,
            rect,
            unavailable: false,
            target,
        });
    }
}

async fn bounded<T>(
    shared: &Shared,
    operation: impl std::future::Future<Output = Result<T, ()>>,
) -> Result<T, ()> {
    tokio::select! {
        result=tokio::time::timeout(HARD_TIMEOUT,operation)=>result.map_err(|_|())?,
        _=async { loop { if shared.cancelled.load(Ordering::Acquire) {break;} tokio::time::sleep(Duration::from_millis(10)).await; } }=>Err(()),
    }
}

async fn exchange(pipe: &mut NamedPipeClient, request: Value) -> Result<Value, ()> {
    let mut request = serde_json::to_vec(&request).map_err(|_| ())?;
    request.push(b'\n');
    pipe.write_all(&request).await.map_err(|_| ())?;
    read_response(pipe).await
}

async fn read_response(reader: &mut (impl tokio::io::AsyncRead + Unpin)) -> Result<Value, ()> {
    let mut response = Vec::new();
    let mut bytes = [0u8; 2048];
    loop {
        let n = reader.read(&mut bytes).await.map_err(|_| ())?;
        if n == 0 || response.len() + n > MAX_RESPONSE {
            return Err(());
        }
        response.extend_from_slice(&bytes[..n]);
        if let Some(end) = response.iter().position(|v| *v == b'\n') {
            if response[end + 1..].iter().any(|v| !v.is_ascii_whitespace()) {
                return Err(());
            }
            return serde_json::from_slice(&response[..end]).map_err(|_| ());
        }
    }
}

fn decode_reply(value: Value, id: u64, point: PhysicalPoint) -> Result<Option<PhysicalRect>, ()> {
    if value["jsonrpc"] != "2.0" || value["id"].as_u64() != Some(id) || value.get("error").is_some()
    {
        return Err(());
    }
    let result = &value["result"];
    if result["coordinate_space"] != "screen_physical" {
        return Err(());
    }
    match result["status"].as_str() {
        Some("no_hit" | "timeout" | "unavailable") => Ok(None),
        Some("hit") => {
            if result["backend"] != "uia"
                || result["process_id"]
                    .as_u64()
                    .filter(|p| *p > 0 && *p <= u32::MAX as u64)
                    .is_none()
                || result["root_hwnd"]
                    .as_str()
                    .and_then(|v| v.parse::<isize>().ok())
                    .filter(|h| *h != 0)
                    .is_none()
            {
                return Err(());
            }
            let number = |name: &str| {
                result["rect"][name]
                    .as_i64()
                    .and_then(|v| i32::try_from(v).ok())
                    .ok_or(())
            };
            let (x, y, w, h) = (
                number("x")?,
                number("y")?,
                number("width")?,
                number("height")?,
            );
            if w <= 0 || h <= 0 {
                return Err(());
            }
            let rect = PhysicalRect {
                left: x,
                top: y,
                right: x.checked_add(w).ok_or(())?,
                bottom: y.checked_add(h).ok_or(())?,
            };
            if !rect.contains(point) {
                return Err(());
            }
            Ok(Some(rect))
        }
        _ => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn hit() -> Value {
        json!({"jsonrpc":"2.0","id":4,"result":{"status":"hit",
            "coordinate_space":"screen_physical","rect":{"x":-20,"y":10,"width":30,"height":40},
            "root_hwnd":"42","process_id":12,"backend":"uia"}})
    }
    #[test]
    fn decodes_physical_rectangle_and_rejects_wrong_identity_or_geometry() {
        let p = PhysicalPoint { x: -1, y: 20 };
        assert_eq!(
            decode_reply(hit(), 4, p),
            Ok(Some(PhysicalRect {
                left: -20,
                top: 10,
                right: 10,
                bottom: 50
            }))
        );
        assert!(decode_reply(hit(), 5, p).is_err());
        assert!(decode_reply(hit(), 4, PhysicalPoint { x: 10, y: 20 }).is_err());
        for (field, value) in [
            ("width", json!(0)),
            ("height", json!(-1)),
            ("x", json!(i32::MAX)),
            ("width", json!(1.5)),
        ] {
            let mut response = hit();
            response["result"]["rect"][field] = value;
            assert!(decode_reply(response, 4, p).is_err());
        }
    }
    #[test]
    fn no_hit_and_timeout_clear_candidate_but_protocol_error_is_failure() {
        for status in ["no_hit", "timeout", "unavailable"] {
            assert_eq!(
                decode_reply(
                    json!({"jsonrpc":"2.0","id":4,"result":{"status":status,"coordinate_space":"screen_physical"}}),
                    4,
                    PhysicalPoint::default()
                ),
                Ok(None)
            );
        }
        assert!(decode_reply(
            json!({"jsonrpc":"2.0","id":4,"error":{"code":-32601}}),
            4,
            PhysicalPoint::default()
        )
        .is_err());
    }
    #[test]
    fn in_flight_a_keeps_only_latest_pending_position_and_failure_does_not_queue() {
        let mut m = Mailbox::default();
        let a = m.request(PhysicalPoint { x: 1, y: 1 });
        let in_flight = m.pending.take().unwrap();
        m.request(PhysicalPoint { x: 2, y: 2 });
        let c = m.request(PhysicalPoint { x: 3, y: 3 });
        assert_eq!(in_flight.sequence, a);
        assert_eq!(
            m.pending.take().unwrap(),
            Request {
                sequence: c,
                point: PhysicalPoint { x: 3, y: 3 }
            }
        );
        m.fail();
        let d = m.request(PhysicalPoint { x: 4, y: 4 });
        assert!(m.pending.is_none());
        assert_eq!(m.reply.unwrap().sequence, d);
        assert!(m.reply.unwrap().unavailable);
    }
    #[tokio::test]
    async fn response_reader_bounds_memory_and_rejects_multiple_messages() {
        let mut valid = &b"{\"id\":1}\n"[..];
        assert_eq!(read_response(&mut valid).await.unwrap()["id"], 1);
        let mut multiple = &b"{}\n{}\n"[..];
        assert!(read_response(&mut multiple).await.is_err());
        let oversized = vec![b' '; MAX_RESPONSE + 1];
        assert!(read_response(&mut oversized.as_slice()).await.is_err());
    }
    #[tokio::test]
    async fn cancellation_interrupts_unresponsive_io() {
        let shared = Arc::new(Shared::default());
        let other = shared.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            other.cancelled.store(true, Ordering::Release);
        });
        let before = Instant::now();
        let result: Result<(), ()> = bounded(&shared, std::future::pending()).await;
        assert!(result.is_err());
        assert!(before.elapsed() < HARD_TIMEOUT);
    }

    #[test]
    #[ignore = "requires freshly built independent Provider and its controlled native fixture executable"]
    fn real_provider_roundtrip_hits_fixture_behind_overlay() {
        let exe =
            PathBuf::from(std::env::var_os("UIPILOT_TEST_PROVIDER_EXE").expect("provider asset"));
        let fixture_exe =
            std::env::var_os("UIPILOT_PROBE_FIXTURE_TEST_EXE").expect("fixture test exe");
        let path =
            std::env::temp_dir().join(format!("uipilot-probe-fixture-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let _fixture = OwnedChild(
            Command::new(fixture_exe)
                .args([
                    "--exact",
                    "point_probe::native_tests::point_probe_native_fixture",
                    "--ignored",
                    "--nocapture",
                ])
                .env("POINT_PROBE_FIXTURE", &path)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(0x08000000)
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        let fixture: Value = loop {
            if let Ok(data) = std::fs::read(&path) {
                if let Ok(value) = serde_json::from_slice(&data) {
                    break value;
                }
            }
            assert!(Instant::now() < deadline, "fixture did not start");
            std::thread::sleep(Duration::from_millis(10));
        };
        let worker = ProbeWorker::start(
            exe,
            vec![fixture["overlay"].as_str().unwrap().parse().unwrap()],
        );
        let mut sequence = worker.request(PhysicalPoint { x: 80, y: 80 });
        let reply = loop {
            assert!(Instant::now() < deadline, "worker did not produce a hit");
            if let Some(reply) = worker.poll() {
                assert!(!reply.unavailable, "Provider handshake/transport failed");
                assert_eq!(reply.sequence, sequence);
                if reply.rect.is_some() {
                    break reply;
                }
                sequence = worker.request(PhysicalPoint { x: 80, y: 80 });
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(!reply.unavailable, "Provider handshake/transport failed");
        assert_eq!(reply.sequence, sequence);
        assert_eq!(
            reply.rect,
            Some(PhysicalRect {
                left: 50,
                top: 50,
                right: 150,
                bottom: 90
            })
        );
        assert_eq!(
            reply.target.unwrap().hwnd.to_string(),
            fixture["root"].as_str().unwrap()
        );
        assert!(target_is_current(reply.target.unwrap()));
        drop(worker);
        let _ = std::fs::remove_file(path);
    }
}
