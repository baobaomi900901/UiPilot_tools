// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(debug_assertions)]
    eprintln!(
        "[UiPilot dev] 主程序进程已启动（PID {}），正在初始化…",
        std::process::id()
    );
    uipilot_lib::prepare_windows_identity();
    uipilot_lib::run();
}
