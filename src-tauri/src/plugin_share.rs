//! Host-only sharing protocol. Native integration is deliberately a separate layer.
#![allow(dead_code)] // Integrated by the subsequent Panel/bridge tasks.

pub(crate) mod catalog;
mod coordinator;
mod delivery;
mod lifecycle;
mod payload;
mod preparation;
#[cfg(test)]
mod tests;
pub(crate) mod types;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ShareError {
    InvalidPayload,
    EnvelopeTooLarge,
    InvalidRevision,
    InvalidIdentity,
    InvalidTicket,
    InvalidState,
    Expired,
    Unavailable,
    TooLarge,
    EntropyUnavailable,
}

#[allow(unused_imports)]
pub(crate) use coordinator::ShareCoordinator;
#[allow(unused_imports)]
pub(crate) use payload::{compact_envelope, Payload};
#[allow(unused_imports)]
pub(crate) use types::*;
