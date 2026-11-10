//! Shared diagnostic pipe runner for the probe and macOS host tests.
use super::{validate_mac_json, DataCall, PluginRequestContext, ReplyPermit};
use serde::Deserialize;
use serde_json::Value;
use std::{
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
pub(super) enum Frame {
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
pub(super) struct Response {
    pub(super) request_id: String,
    pub(super) data: String,
}
pub(super) enum Event {
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
pub(super) struct Pipe {
    child: OwnedChild,
    writers: Option<SyncSender<WriteJob>>,
    events: Option<Receiver<Event>>,
    threads: Vec<JoinHandle<()>>,
    pub(super) deadline: Instant,
    reaped: bool,
    sent: usize,
    cancelled: Box<dyn Fn() -> bool + Send>,
}
impl Pipe {
    pub(super) fn new(
        path: &Path,
        deadline: Instant,
        cancelled: impl Fn() -> bool + Send + 'static,
    ) -> Result<Self, ()> {
        if cancelled() || Instant::now() >= deadline {
            return Err(());
        }
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
        let mut pipe = Self {
            child,
            writers: Some(writer_tx),
            events: Some(rx),
            threads: Vec::new(),
            deadline,
            reaped: false,
            sent: 0,
            cancelled: Box::new(cancelled),
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
                if validate_mac_json(&bytes).is_none() {
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
        pipe.threads.push(reader);
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
        pipe.threads.push(stderr);
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
        pipe.threads.push(writer);
        Ok(pipe)
    }
    pub(super) fn pid(&self) -> u32 {
        self.child.id()
    }
    fn remaining(&self) -> Result<Duration, ()> {
        if (self.cancelled)() {
            return Err(());
        }
        self.deadline
            .checked_duration_since(Instant::now())
            .ok_or(())
    }
    pub(super) fn write(&mut self, value: Value, permit: Option<ReplyPermit>) -> Result<(), ()> {
        self.remaining()?;
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
        if self.receive(&rx)? {
            Ok(())
        } else {
            Err(())
        }
    }
    pub(super) fn event(&self) -> Result<Event, ()> {
        self.receive(self.events.as_ref().ok_or(())?)
    }
    fn receive<T>(&self, receiver: &Receiver<T>) -> Result<T, ()> {
        loop {
            let interval = self.remaining()?.min(Duration::from_millis(10));
            match receiver.recv_timeout(interval) {
                Ok(value) => return Ok(value),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err(()),
            }
        }
    }
    pub(super) fn terminal(&mut self, mut stderr_eof: bool) -> Result<(), ()> {
        // Completion cannot be followed by another frame or an unsuccessful exit.
        let mut stdout_eof = false;
        while !stdout_eof || !stderr_eof {
            match self.event()? {
                Event::Eof if !stdout_eof => stdout_eof = true,
                Event::StderrEof if !stderr_eof => stderr_eof = true,
                _ => return Err(()),
            }
        }
        loop {
            self.remaining()?;
            if let Some(status) = self.child.try_wait().map_err(|_| ())? {
                self.reaped = true;
                if !status.success() {
                    return Err(());
                }
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    }
    pub(super) fn stop(&mut self) -> bool {
        if !self.reaped {
            let _ = self.child.kill();
            self.reaped = self.child.wait().is_ok();
        }
        drop(self.writers.take());
        drop(self.events.take());
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
        self.reaped
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        self.stop();
    }
}
