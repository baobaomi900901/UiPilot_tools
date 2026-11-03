//! Dev-only startup breadcrumbs. No user content or configuration values are recorded.
#[cfg(debug_assertions)]
mod debug {
    use std::{fs::OpenOptions, io::Write, sync::{Mutex, OnceLock}, time::Instant};
    struct Progress { label: &'static str, since: Instant, done: bool }
    static PROGRESS: OnceLock<Mutex<Progress>> = OnceLock::new();
    static START: OnceLock<Instant> = OnceLock::new();

    pub fn note(message: &str) {
        let elapsed = START.get_or_init(Instant::now).elapsed().as_secs_f32();
        let line = format!("[UiPilot dev +{elapsed:.1}s PID {}] {message}", std::process::id());
        eprintln!("{line}");
        if let Some(path) = std::env::var_os("UIPILOT_DEV_STARTUP_LOG") {
            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(file, "{line}");
            }
        }
    }

    pub fn stage(label: &'static str, done: bool) {
        let progress = PROGRESS.get_or_init(|| {
            let _ = std::thread::Builder::new().name("dev-startup-watch".into()).spawn(|| loop {
                std::thread::sleep(std::time::Duration::from_secs(15));
                let Some(progress) = PROGRESS.get() else { continue };
                let Ok(current) = progress.try_lock() else { continue };
                if current.done { break }
                let message = format!("仍在等待：{}（本阶段 {:.1} 秒）", current.label, current.since.elapsed().as_secs_f32());
                drop(current);
                note(&message);
            });
            Mutex::new(Progress { label, since: Instant::now(), done: false })
        });
        if let Ok(mut current) = progress.lock() {
            if current.done { return }
            *current = Progress { label, since: Instant::now(), done };
        }
        note(label);
    }
}

pub(crate) fn stage(label: &'static str) {
    #[cfg(debug_assertions)] debug::stage(label, false);
    #[cfg(not(debug_assertions))] let _ = label;
}
#[cfg(debug_assertions)]
pub(crate) fn ready() {
    #[cfg(debug_assertions)] debug::stage("启动诊断完成：主程序已就绪", true);
}
pub(crate) fn note(message: &str) {
    #[cfg(debug_assertions)] debug::note(message);
    #[cfg(not(debug_assertions))] let _ = message;
}
