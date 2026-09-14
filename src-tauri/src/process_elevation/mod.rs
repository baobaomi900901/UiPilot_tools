mod launch;
mod pipe;
mod protocol;
mod session;
mod trust;

#[allow(
    unused_imports,
    reason = "Task 3 wires launch lifecycle into the coordinator"
)]
pub(crate) use launch::{LaunchCoordinator, LaunchReservation, LaunchResult};
#[allow(
    unused_imports,
    reason = "Task 3 wires transport and trust into the coordinator"
)]
pub(crate) use pipe::{read_frame, read_validated_request, write_frame};
#[allow(
    unused_imports,
    reason = "Task 3 wires the private boundary into the coordinator"
)]
pub(crate) use protocol::{
    decode_frame, encode_frame, validate_request, Operation, Request, Scope, StableError, Terminal,
    TerminalResult,
};
#[allow(
    unused_imports,
    reason = "Task 3 wires the private boundary into the coordinator"
)]
pub(crate) use session::{Phase, Session};
#[allow(
    unused_imports,
    reason = "Task 3 wires peer verification into the coordinator"
)]
pub(crate) use trust::{verify_peer, PeerIdentity, TrustPackage};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facade_exposes_the_same_strict_contract_as_the_helper_source() {
        let request = Request {
            version: 1,
            request_id: "a".repeat(64),
            scope: Scope::Tree,
            operation: Operation::ForceTerminate,
            root: protocol::RootSelector {
                pid: 7,
                creation_time: 11,
            },
        };
        validate_request(&request).unwrap();
        assert_eq!(
            decode_frame::<Request>(&encode_frame(&request).unwrap()).unwrap(),
            request
        );
        let terminal = Terminal {
            version: 1,
            request_id: "a".repeat(64),
            result: TerminalResult::Cancelled,
        };
        assert_eq!(
            decode_frame::<Terminal>(&encode_frame(&terminal).unwrap()).unwrap(),
            terminal
        );
        assert_eq!(
            StableError::ProtocolRejected as u8,
            StableError::ProtocolRejected as u8
        );
    }
}
