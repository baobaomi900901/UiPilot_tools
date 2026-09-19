#[cfg(windows)]
#[path = "../../../src/process_elevation/image.rs"]
pub(crate) mod image;
#[cfg(windows)]
#[path = "../../../src/process_elevation/peer.rs"]
pub(crate) mod peer;
#[cfg(windows)]
#[path = "../../../src/process_elevation/permissions.rs"]
pub(crate) mod permissions;
#[path = "../../../src/process_elevation/pipe.rs"]
pub(crate) mod pipe;
#[path = "../../../src/process_elevation/protocol.rs"]
pub(crate) mod protocol;
#[path = "../../../src/process_elevation/session.rs"]
pub(crate) mod session;
#[cfg(windows)]
#[path = "../../../src/process_elevation/token.rs"]
pub(crate) mod token;
#[path = "../../../src/process_elevation/trust.rs"]
pub(crate) mod trust;
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
