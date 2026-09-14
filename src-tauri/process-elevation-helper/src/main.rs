#[allow(
    dead_code,
    reason = "Task 3 helper compiles the same private transport boundary"
)]
mod process_elevation;

fn main() {
    // Task 2 deliberately ships a fail-closed helper shell. Task 3 owns launch,
    // peer authentication and the first request; no privileged operation is reachable.
    std::process::exit(2);
}
