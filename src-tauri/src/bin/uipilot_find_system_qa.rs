//! Standalone QA executable. A Finder double-click must not initiate a global query.
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let action = args.next();
    let action = action.as_deref().and_then(std::ffi::OsStr::to_str);
    if !matches!(action, Some("--check-workspace" | "--run-isolated")) || args.next().is_some() {
        eprintln!("Use run-spotlight-qa.command from a dedicated macOS QA account.");
        return ExitCode::from(2);
    }
    #[cfg(target_os = "macos")]
    {
        if action == Some("--check-workspace") {
            return match uipilot_lib::qa_shared_workspace_identity() {
                Ok(identity) => { println!("{identity}"); ExitCode::SUCCESS }
                Err(reason) => { eprintln!("QA preflight failed: {reason}"); ExitCode::FAILURE }
            };
        }
        match uipilot_lib::run_system_qa_probe() {
            Ok(()) => ExitCode::SUCCESS,
            Err(reason) => { eprintln!("QA did not pass: {reason}"); ExitCode::FAILURE }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("This probe requires macOS on Apple Silicon.");
        ExitCode::from(2)
    }
}
