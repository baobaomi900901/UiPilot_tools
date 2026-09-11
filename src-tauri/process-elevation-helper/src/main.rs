#[path = "../../src/process_elevation/protocol.rs"]
#[allow(
    dead_code,
    reason = "Task 3 wires protocol handling into this fail-closed shell"
)]
mod protocol;
#[path = "../../src/process_elevation/session.rs"]
#[allow(
    dead_code,
    reason = "Task 3 wires lifecycle handling into this fail-closed shell"
)]
mod session;

fn main() {
    // Task 2 deliberately ships a fail-closed helper shell. Task 3 owns launch,
    // peer authentication and the first request; no privileged operation is reachable.
    std::process::exit(2);
}
