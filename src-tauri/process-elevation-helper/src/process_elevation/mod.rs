#[cfg(windows)]
#[path = "../../../src/process_elevation/peer.rs"]
pub(crate) mod peer;
#[path = "../../../src/process_elevation/pipe.rs"]
pub(crate) mod pipe;
#[path = "../../../src/process_elevation/protocol.rs"]
pub(crate) mod protocol;
#[path = "../../../src/process_elevation/session.rs"]
pub(crate) mod session;
#[allow(
    unused_imports,
    reason = "Task 3 wires these exports into the helper session"
)]
pub(crate) use pipe::{read_frame, read_validated_request, write_frame};
#[allow(
    unused_imports,
    reason = "Task 3 wires these exports into the helper session"
)]
pub(crate) use protocol::{
    decode_frame, encode_frame, validate_request, Operation, Request, Scope, StableError, Terminal,
    TerminalResult,
};
#[allow(
    unused_imports,
    reason = "Task 3 wires these exports into the helper session"
)]
pub(crate) use session::{Phase, Session};
