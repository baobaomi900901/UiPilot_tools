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

const MAX_RESPONSE: usize = 4 * 1024 * 1024;
const MAX_NODES: usize = 2048;
const MAX_DEPTH: usize = 64;
const MAX_CACHED_WINDOWS: usize = 8;
const HARD_TIMEOUT: Duration = Duration::from_secs(3);
const MIN_INTERVAL: Duration = Duration::from_millis(50);

pub(crate) fn trace(message: impl AsRef<str>) {
    #[cfg(debug_assertions)]
    {
        use std::io::Write;
        static LOCK: Mutex<()> = Mutex::new(());
        if let Ok(_lock) = LOCK.try_lock() {
            let millis = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let line = format!("[screenshot {millis}] {}", message.as_ref());
            eprintln!("{line}");
            let path =
                std::env::temp_dir().join(format!("uipilot-screenshot-{}.log", std::process::id()));
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(file, "{line}");
            }
        }
    }
    #[cfg(not(debug_assertions))]
    let _ = message;
}

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
    notify: Option<Box<dyn Fn() + Send + Sync>>,
}
impl Shared {
    fn notify_reply(&self) {
        if !self.cancelled.load(Ordering::Acquire) {
            if let Some(notify) = &self.notify {
                notify();
            }
        }
    }
}

pub(crate) struct ProbeWorker {
    shared: Arc<Shared>,
}
impl ProbeWorker {
    pub fn start(
        exe: PathBuf,
        excluded: Vec<isize>,
        notify: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            notify: Some(Box::new(notify)),
            ..Shared::default()
        });
        let worker = shared.clone();
        if std::thread::Builder::new()
            .name("screenshot-probe".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_worker(exe, excluded, &worker)
                }));
                if !worker.cancelled.load(Ordering::Acquire) && !matches!(result, Ok(Ok(()))) {
                    trace("provider transport/startup failed; window selection remains available");
                    if let Ok(mut mailbox) = worker.mailbox.lock() {
                        mailbox.fail();
                    }
                    worker.notify_reply();
                }
            })
            .is_err()
        {
            shared.mailbox.lock().unwrap().fail();
            shared.notify_reply();
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
    if manifest["schema_version"] != 1 || !tree_capabilities(&manifest["capabilities"]) {
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

fn tree_capabilities(capabilities: &Value) -> bool {
    capabilities["win32_elementtree"] == true
        && capabilities["win32_inspect_tree_v2"] == true
        && capabilities["win32_observation_sessions"] == true
}

fn provider_command(exe: &Path) -> Command {
    let mut command = Command::new(exe);
    // Original Provider configuration: an empty object restores its defaults,
    // but a nonempty map with empty target lists disables every helper match.
    // Override only this owned child, including any inherited custom mapping.
    command.env("UIAUTOMA_HELPER_INJECT_MAP", r#"{"weixin":[],"wxwork":[]}"#);
    command
}

// This is native window selection only. UIA remains in the independent Provider.
fn target_at(point: PhysicalPoint, excluded: &[isize], provider_pid: u32) -> Option<ProbeTarget> {
    use windows::{
        core::BOOL,
        Win32::{
            Foundation::{HWND, LPARAM},
            Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED},
            UI::WindowsAndMessaging::EnumWindows,
        },
    };
    struct Search<'a> {
        point: PhysicalPoint,
        excluded: &'a [isize],
        provider_pid: u32,
        target: Option<ProbeTarget>,
    }
    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
        let search = &mut *(data.0 as *mut Search<'_>);
        if search.excluded.contains(&(hwnd.0 as isize)) {
            return BOOL(1);
        }
        let Some(target) = snapshot_target(hwnd.0 as isize) else {
            return BOOL(1);
        };
        if target.pid == std::process::id()
            || target.pid == search.provider_pid
            || !target.rect.contains(search.point)
        {
            return BOOL(1);
        }
        let mut cloaked = 0u32;
        if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4).is_ok()
            && cloaked != 0
        {
            return BOOL(1);
        }
        search.target = Some(target);
        BOOL(0)
    }
    let mut search = Search {
        point,
        excluded,
        provider_pid,
        target: None,
    };
    // EnumWindows returns false when the callback intentionally stops at the first match.
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM(&mut search as *mut _ as isize));
    }
    search.target
}

struct GeometryCache {
    target: ProbeTarget,
    // Empty also records a failed/incomplete collection; never retry every mouse move.
    rectangles: Vec<PhysicalRect>,
}

async fn tree_rpc(pipe: &mut NamedPipeClient, id: &mut u64, params: Value) -> Result<Value, ()> {
    *id = id.checked_add(1).ok_or(())?;
    let response = exchange(
        pipe,
        json!({"jsonrpc":"2.0","id":*id,"method":"elementtree","params":params}),
    )
    .await?;
    if response["jsonrpc"] != "2.0"
        || response["id"].as_u64() != Some(*id)
        || response.get("error").is_some()
    {
        return Err(());
    }
    response.get("result").cloned().ok_or(())
}

async fn collect_geometry(
    pipe: &mut NamedPipeClient,
    id: &mut u64,
    target: ProbeTarget,
) -> Result<Vec<PhysicalRect>, ()> {
    let opened = tree_rpc(pipe, id, json!({"operation":"open_session","backend":"uia",
        "interactive":false,"public_observation":true,"native_handle":target.hwnd,"collection_budget_ms":2500})).await?;
    if opened["error"] != 0 {
        return Ok(vec![]);
    }
    let session = opened["session_id"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128)
        .ok_or(())?;
    let snapshot = opened["snapshot_id"]
        .as_i64()
        .filter(|s| *s > 0)
        .ok_or(())?;
    let valid_root = opened["backend"] == "uia"
        && opened["display"]["native_window_handle"].as_i64() == Some(target.hwnd as i64)
        && opened["display"]["process_id"].as_u64() == Some(u64::from(target.pid));
    let rectangles = if valid_root {
        let tree = tree_rpc(
            pipe,
            id,
            json!({"operation":"get_subtree","session_id":session,
            "snapshot_id":snapshot,"path_from_root":[],"public_observation":true,
            "max_nodes":MAX_NODES,"max_depth":MAX_DEPTH,"collection_budget_ms":2500}),
        )
        .await?;
        decode_tree(&tree, snapshot).unwrap_or_default()
    } else {
        vec![]
    };
    // No live session or element metadata survives the one-time collection.
    let closed = tree_rpc(
        pipe,
        id,
        json!({"operation":"close_session","session_id":session}),
    )
    .await?;
    if closed["error"] != 0 || closed["closed"] != true {
        return Err(());
    }
    Ok(rectangles)
}

fn run_worker(exe: PathBuf, excluded: Vec<isize>, shared: &Shared) -> Result<(), ()> {
    // Native target snapshots and Provider geometry use physical desktop pixels.
    unsafe {
        windows::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
    trace(format!("provider validating {}", exe.display()));
    verify_asset(&exe)?;
    if shared.cancelled.load(Ordering::Acquire) {
        return Ok(());
    }
    let nonce = unsafe { windows::Win32::System::Com::CoCreateGuid() }.map_err(|_| ())?;
    let pipe_name = format!(r"\\.\pipe\UiPilot.Screenshot.{nonce:?}");
    let mut command = provider_command(&exe);
    command.stderr(Stdio::null());
    let mut child = OwnedChild(
        command
            .args([
                "--pipe-name",
                &pipe_name,
                "--parent-pid",
                &std::process::id().to_string(),
            ])
            .current_dir(exe.parent().ok_or(())?)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .creation_flags(0x08000000)
            .spawn()
            .map_err(|_| ())?,
    );
    trace("provider child spawned");
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
    trace("provider pipe connected");
    let ping = runtime.block_on(bounded(
        shared,
        exchange(
            &mut pipe,
            json!({"jsonrpc":"2.0","id":0,"method":"system.ping","params":{}}),
        ),
    ))?;
    if ping["jsonrpc"] != "2.0"
        || ping["id"] != 0
        || ping.get("error").is_some()
        || !tree_capabilities(&ping["result"]["capabilities"])
    {
        return Err(());
    }
    let mut last = Instant::now()
        .checked_sub(MIN_INTERVAL)
        .unwrap_or_else(Instant::now);
    let mut rpc_id = 0;
    let mut cache: Vec<GeometryCache> = Vec::new();
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
        let target = target_at(request.point, &excluded, child.0.id());
        if let Some(target) = target {
            if !cache.iter().any(|entry| entry.target == target) {
                cache.retain(|entry| entry.target.hwnd != target.hwnd);
                if cache.len() >= MAX_CACHED_WINDOWS {
                    cache.remove(0);
                }
                let rectangles = runtime.block_on(bounded(
                    shared,
                    collect_geometry(&mut pipe, &mut rpc_id, target),
                ))?;
                trace(format!(
                    "provider tree collected rectangles={}",
                    rectangles.len()
                ));
                cache.push(GeometryCache { target, rectangles });
            }
        }
        // A window can move/close/change stacking while the Provider is collecting.
        let target = target
            .filter(|target| target_at(request.point, &excluded, child.0.id()) == Some(*target));
        let rect = target
            .and_then(|target| cache.iter().find(|entry| entry.target == target))
            .and_then(|entry| smallest_at(&entry.rectangles, request.point, entry.target.rect));
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
        drop(mailbox);
        trace(format!(
            "probe timing sequence={} elapsed_ms={}",
            request.sequence,
            last.elapsed().as_millis()
        ));
        shared.notify_reply();
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
        let offset = response.len();
        response.extend_from_slice(&bytes[..n]);
        if let Some(end) = bytes[..n]
            .iter()
            .position(|v| *v == b'\n')
            .map(|end| offset + end)
        {
            if response[end + 1..].iter().any(|v| !v.is_ascii_whitespace()) {
                return Err(());
            }
            return serde_json::from_slice(&response[..end]).map_err(|_| ());
        }
    }
}

// Consume the original flat parent-batch format. Reject incomplete trees rather
// than advertising the smallest rectangle from only a prefix of the UIA tree.
fn decode_tree(value: &Value, snapshot: i64) -> Result<Vec<PhysicalRect>, ()> {
    use std::collections::{BTreeMap, BTreeSet};
    if value["error"] != 0
        || value["snapshot_id"].as_i64() != Some(snapshot)
        || value["truncated"] != false
        || value["budget_exhausted"] != false
        || value["root_path_from_root"] != json!([])
    {
        return Err(());
    }
    let parents = value["parents"].as_array().ok_or(())?;
    if parents.is_empty()
        || parents.len() > MAX_NODES + 1
        || value["parent_count"].as_u64() != Some(parents.len() as u64)
    {
        return Err(());
    }
    let mut expected = BTreeSet::from([Vec::<usize>::new()]);
    let mut seen = BTreeSet::new();
    let mut nodes = 0usize;
    let mut rectangles = BTreeMap::new();
    for parent in parents {
        let path = parent["path_from_root"]
            .as_array()
            .ok_or(())?
            .iter()
            .map(|v| {
                v.as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .filter(|n| *n < MAX_NODES)
                    .ok_or(())
            })
            .collect::<Result<Vec<_>, _>>()?;
        if path.len() >= MAX_DEPTH
            || !expected.remove(&path)
            || !seen.insert(path.clone())
            || parent["truncated"] != false
        {
            return Err(());
        }
        let children = parent["children"].as_array().ok_or(())?;
        if parent["count"].as_u64() != Some(children.len() as u64) {
            return Err(());
        }
        nodes = nodes.checked_add(children.len()).ok_or(())?;
        if nodes > MAX_NODES {
            return Err(());
        }
        for (index, child) in children.iter().enumerate() {
            if child["ref"]["snapshot_id"].as_i64() != Some(snapshot)
                || child["ref"]["platform"] != "win32"
                || child["ref"]["automation_type"] != "UIA"
            {
                return Err(());
            }
            if child["has_children"].as_bool().ok_or(())? {
                let mut child_path = path.clone();
                child_path.push(index);
                expected.insert(child_path);
            }
            let display = &child["display"];
            // Never prune by parent geometry: TreeItem and logical containers can
            // expose children whose screen bounds lie outside the parent's bounds.
            if display["is_offscreen"].as_bool().ok_or(())? {
                continue;
            }
            let geometry = &display["position_and_shape"];
            if geometry.is_null() {
                continue;
            }
            let number = |key: &str| {
                geometry[key]
                    .as_i64()
                    .and_then(|n| i32::try_from(n).ok())
                    .ok_or(())
            };
            let (x, y, w, h) = (number("x")?, number("y")?, number("w")?, number("h")?);
            if w <= 0 || h <= 0 {
                continue;
            }
            let rect = PhysicalRect {
                left: x,
                top: y,
                right: x.checked_add(w).ok_or(())?,
                bottom: y.checked_add(h).ok_or(())?,
            };
            rectangles.insert((rect.left, rect.top, rect.right, rect.bottom), rect);
        }
    }
    if !expected.is_empty() || value["count"].as_u64() != Some(nodes as u64) {
        return Err(());
    }
    Ok(rectangles.into_values().collect())
}

fn smallest_at(
    rectangles: &[PhysicalRect],
    point: PhysicalPoint,
    bounds: PhysicalRect,
) -> Option<PhysicalRect> {
    let mut selected = None;
    let mut minimum = i64::MAX;
    let mut ambiguous = false;
    for rect in rectangles.iter().copied().filter(|rect| {
        rect.contains(point)
            && rect.left >= bounds.left
            && rect.top >= bounds.top
            && rect.right <= bounds.right
            && rect.bottom <= bounds.bottom
    }) {
        let area = (i64::from(rect.right) - i64::from(rect.left))
            * (i64::from(rect.bottom) - i64::from(rect.top));
        if area < minimum {
            selected = Some(rect);
            minimum = area;
            ambiguous = false;
        } else if area == minimum && selected != Some(rect) {
            ambiguous = true;
        }
    }
    selected.filter(|_| !ambiguous)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn original_tree() -> Value {
        let item = |x, y, w, h, children| {
            json!({
                "ref":{"snapshot_id":1,"platform":"win32","automation_type":"UIA"},
                "display":{"position_and_shape":{"x":x,"y":y,"w":w,"h":h},"is_offscreen":false},
                "has_children":children
            })
        };
        json!({"error":0,"snapshot_id":1,"root_path_from_root":[],"count":3,
        "parent_count":2,"truncated":false,"budget_exhausted":false,"parents":[
            {"path_from_root":[],"count":1,"truncated":false,"children":[item(0,0,10,10,true)]},
            {"path_from_root":[0],"count":2,"truncated":false,"children":[item(50,50,20,20,false),item(50,50,20,20,false)]}
        ]})
    }
    #[test]
    fn original_tree_geometry_includes_descendants_outside_parent_and_deduplicates() {
        let geometry = decode_tree(&original_tree(), 1).unwrap();
        assert_eq!(geometry.len(), 2);
        assert_eq!(
            smallest_at(
                &geometry,
                PhysicalPoint { x: 55, y: 55 },
                PhysicalRect {
                    left: 0,
                    top: 0,
                    right: 100,
                    bottom: 100
                }
            ),
            Some(PhysicalRect {
                left: 50,
                top: 50,
                right: 70,
                bottom: 70
            })
        );
    }
    #[test]
    fn incomplete_or_stale_original_tree_cannot_claim_smallest_element() {
        for key in ["truncated", "budget_exhausted"] {
            let mut tree = original_tree();
            tree[key] = json!(true);
            assert!(decode_tree(&tree, 1).is_err());
        }
        let mut tree = original_tree();
        tree["parents"][1]["truncated"] = json!(true);
        assert!(decode_tree(&tree, 1).is_err());
        assert!(decode_tree(&original_tree(), 2).is_err());
        let mut tree = original_tree();
        tree["parents"].as_array_mut().unwrap().pop();
        assert!(decode_tree(&tree, 1).is_err());
    }
    #[test]
    fn original_tree_skips_empty_and_offscreen_geometry_but_rejects_overflow() {
        let mut tree = original_tree();
        tree["parents"][1]["children"][0]["display"]["position_and_shape"]["w"] = json!(0);
        tree["parents"][1]["children"][1]["display"]["is_offscreen"] = json!(true);
        assert_eq!(decode_tree(&tree, 1).unwrap().len(), 1);
        tree["parents"][0]["children"][0]["display"]["position_and_shape"]["x"] = json!(i32::MAX);
        assert!(decode_tree(&tree, 1).is_err());
    }
    #[test]
    fn original_tree_capabilities_do_not_require_a_custom_point_api() {
        assert!(tree_capabilities(
            &json!({"win32_elementtree":true,"win32_inspect_tree_v2":true,"win32_observation_sessions":true})
        ));
        for missing in [
            "win32_elementtree",
            "win32_inspect_tree_v2",
            "win32_observation_sessions",
        ] {
            let mut capabilities = json!({"win32_elementtree":true,"win32_inspect_tree_v2":true,"win32_observation_sessions":true});
            capabilities.as_object_mut().unwrap().remove(missing);
            assert!(
                !tree_capabilities(&capabilities),
                "required capability: {missing}"
            );
        }
    }
    #[test]
    fn provider_child_disables_helper_targets_without_triggering_default_map() {
        let command = provider_command(Path::new("provider.exe"));
        let mapping = command
            .get_envs()
            .find(|(key, _)| *key == "UIAUTOMA_HELPER_INJECT_MAP")
            .and_then(|(_, value)| value)
            .expect("child must override inherited helper injection configuration");
        let mapping: std::collections::HashMap<String, Vec<String>> =
            serde_json::from_str(mapping.to_str().unwrap()).unwrap();
        assert!(
            !mapping.is_empty(),
            "the original Provider replaces an empty map with its defaults"
        );
        assert_eq!(mapping.len(), 2);
        assert!(mapping.contains_key("weixin") && mapping.contains_key("wxwork"));
        assert!(mapping.values().all(Vec::is_empty));
    }
    #[test]
    fn smallest_element_rejects_geometry_outside_native_root() {
        let bounds = PhysicalRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };
        let outside = PhysicalRect {
            left: -5,
            top: 10,
            right: 20,
            bottom: 30,
        };
        assert_eq!(
            smallest_at(&[outside, bounds], PhysicalPoint { x: 10, y: 20 }, bounds),
            Some(bounds)
        );
    }
    #[test]
    fn distinct_equal_minimum_rectangles_are_ambiguous_but_identical_ones_are_not() {
        let bounds = PhysicalRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };
        let a = PhysicalRect {
            left: 10,
            top: 10,
            right: 30,
            bottom: 30,
        };
        let b = PhysicalRect {
            left: 15,
            top: 15,
            right: 35,
            bottom: 35,
        };
        let point = PhysicalPoint { x: 20, y: 20 };
        assert_eq!(smallest_at(&[a, b], point, bounds), None);
        assert_eq!(smallest_at(&[a, a], point, bounds), Some(a));
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
    #[ignore = "controlled native fixture subprocess for the original Provider integration test"]
    fn original_provider_native_fixture() {
        use windows::{
            core::w,
            Win32::{
                Foundation::RECT,
                UI::{HiDpi::*, WindowsAndMessaging::*},
            },
        };
        let path = PathBuf::from(
            std::env::var_os("UIPILOT_ORIGINAL_PROVIDER_FIXTURE").expect("fixture output"),
        );
        unsafe {
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let root = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                w!("STATIC"),
                w!("UiPilot controlled fixture"),
                WS_POPUP,
                40,
                40,
                400,
                300,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let button = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("BUTTON"),
                w!("Fixture"),
                WS_CHILD | WS_VISIBLE,
                20,
                20,
                100,
                40,
                Some(root),
                None,
                None,
                None,
            )
            .unwrap();
            let _ = ShowWindow(root, SW_SHOWNOACTIVATE);
            let overlay = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                w!("STATIC"),
                w!("UiPilot excluded fixture overlay"),
                WS_POPUP,
                0,
                0,
                800,
                600,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let _ = ShowWindow(overlay, SW_SHOWNOACTIVATE);
            let mut rect = RECT::default();
            GetWindowRect(button, &mut rect).unwrap();
            std::fs::write(&path, serde_json::to_vec(&json!({"root":(root.0 as isize).to_string(),
                "overlay":(overlay.0 as isize).to_string(),"rect":{"left":rect.left,"top":rect.top,"right":rect.right,"bottom":rect.bottom}})).unwrap()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            let mut message = MSG::default();
            while Instant::now() < deadline {
                while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let _ = DestroyWindow(overlay);
            let _ = DestroyWindow(root);
        }
    }

    #[test]
    #[ignore = "requires the staged original Provider; opens only the host-owned controlled fixture"]
    fn real_provider_roundtrip_hits_fixture_behind_overlay() {
        let exe =
            PathBuf::from(std::env::var_os("UIPILOT_TEST_PROVIDER_EXE").expect("provider asset"));
        let path = std::env::temp_dir().join(format!(
            "uipilot-original-probe-fixture-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let _fixture = OwnedChild(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "screenshot::probe::tests::original_provider_native_fixture",
                    "--ignored",
                    "--nocapture",
                ])
                .env("UIPILOT_ORIGINAL_PROVIDER_FIXTURE", &path)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(0x08000000)
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(12);
        let fixture: Value = loop {
            if let Ok(data) = std::fs::read(&path) {
                if let Ok(value) = serde_json::from_slice(&data) {
                    break value;
                }
            }
            assert!(Instant::now() < deadline, "fixture did not start");
            std::thread::sleep(Duration::from_millis(10));
        };
        let number = |name: &str| fixture["rect"][name].as_i64().unwrap() as i32;
        let expected = PhysicalRect {
            left: number("left"),
            top: number("top"),
            right: number("right"),
            bottom: number("bottom"),
        };
        let point = PhysicalPoint {
            x: expected.left + 10,
            y: expected.top + 10,
        };
        let excluded = vec![fixture["overlay"].as_str().unwrap().parse().unwrap()];
        let (notifier, notified) = std::sync::mpsc::channel();
        let worker = ProbeWorker::start(exe, excluded, move || {
            let _ = notifier.send(());
        });
        let first_started = Instant::now();
        let sequence = worker.request(point);
        notified
            .recv_timeout(Duration::from_secs(8))
            .expect("reply must wake overlay");
        let reply = worker.poll().expect("notified reply");
        assert!(!reply.unavailable, "Provider handshake/transport failed");
        assert_eq!(reply.sequence, sequence);
        assert_eq!(reply.rect, Some(expected));
        assert_eq!(
            reply.target.unwrap().hwnd.to_string(),
            fixture["root"].as_str().unwrap()
        );
        assert!(target_is_current(reply.target.unwrap()));
        eprintln!(
            "original Provider initial collection: {} ms",
            first_started.elapsed().as_millis()
        );
        let cached_started = Instant::now();
        let sequence = worker.request(PhysicalPoint {
            x: point.x + 2,
            y: point.y + 2,
        });
        notified
            .recv_timeout(Duration::from_secs(2))
            .expect("cached reply");
        let reply = worker.poll().unwrap();
        assert_eq!(reply.sequence, sequence);
        assert_eq!(reply.rect, Some(expected));
        assert!(!reply.unavailable);
        assert!(cached_started.elapsed() < Duration::from_secs(1));
        eprintln!(
            "original Provider cached point: {} ms",
            cached_started.elapsed().as_millis()
        );
        drop(worker);
        let _ = std::fs::remove_file(path);
    }
}
