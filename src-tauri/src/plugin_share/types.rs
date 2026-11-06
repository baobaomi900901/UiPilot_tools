use super::{catalog::Receiver, payload::Payload, ShareError};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub(crate) const SHORT: Duration = Duration::from_secs(2);
pub(crate) const CHOOSING: Duration = Duration::from_secs(120);
pub(crate) const PREPARING: Duration = Duration::from_secs(30);
pub(crate) const RECOVERY: Duration = Duration::from_secs(120);

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PluginIdentity {
    pub(crate) plugin_id: String,
    #[serde(with = "decimal_u64")]
    pub(crate) generation: u64,
    #[serde(with = "decimal_u64")]
    pub(crate) activation_id: u64,
    #[serde(with = "decimal_u64")]
    pub(crate) admission_epoch: u64,
    #[serde(with = "decimal_u64")]
    pub(crate) inventory_revision: u64,
}
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct Document {
    pub(crate) plugin: PluginIdentity,
    pub(crate) webview_label: String,
    pub(crate) nonce: String,
    pub(crate) epoch: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Owner {
    pub(crate) main_webview: String,
    pub(crate) document_nonce: String,
    pub(crate) navigation_epoch: u64,
}
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct Ticket(pub(crate) String);

pub(crate) trait IdSource {
    fn next(&mut self) -> Result<Ticket, ShareError>;
}
pub(crate) struct SecureIds;
impl IdSource for SecureIds {
    fn next(&mut self) -> Result<Ticket, ShareError> {
        #[cfg(windows)]
        {
            use windows::Win32::Security::Cryptography::{
                BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            };
            let mut bytes = [0u8; 32];
            let status =
                unsafe { BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
            if status.0 < 0 {
                return Err(ShareError::EntropyUnavailable);
            }
            Ok(Ticket(bytes.iter().map(|b| format!("{b:02x}")).collect()))
        }
        #[cfg(not(windows))]
        {
            Err(ShareError::EntropyUnavailable)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum State {
    Choosing,
    Preparing,
    Committed,
    Accepted,
    DeliveryFailed,
    DeliveryUnknown,
    Cancelled,
    PrepareFailed,
    Closed,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Effect {
    /// No native/plugin work occurs under the coordinator's gate. Execute effects afterward.
    DestroyPending(Document),
    RetirePreparation {
        transfer: Ticket,
        target: Document,
    },
    RestoreSource(Document),
    RevokeDelivery {
        transfer: Ticket,
        target: Document,
    },
    DestroyTarget(Document),
    EnableTargetInput(Document),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EndReason {
    ContinueEditing,
    CloseRecovery,
    NewShare,
    Navigation,
    Hidden,
    IdentityRevoked,
    RecoveryExpired,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrepareStep {
    Runtime,
    Initialization,
    SourceConfirmation,
    NativeSwitch,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LocalCancellation {
    Revoked,
    AlreadyConsumed,
}

/// Current authoritative facts, read under the SAME gate as Panel routing and
/// lifecycle mutations. These are NOT a security proof from untrusted IPC.
pub(crate) struct CommitView<'a> {
    pub(crate) owner: &'a Owner,
    pub(crate) active_source: &'a Document,
    pub(crate) pending_target: &'a Document,
    pub(crate) receiver: &'a Receiver,
    pub(crate) main_visible: bool,
    pub(crate) initialization: &'a Initialization,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CommitTransition {
    pub(crate) source: Document,
    pub(crate) target: Document,
    pub(crate) transfer: Ticket,
    pub(crate) receiver_id: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Delivery {
    pub(crate) version: u8,
    pub(crate) transfer_id: Ticket,
    pub(crate) receiver_id: String,
    pub(crate) session_epoch: String,
    pub(crate) payload: Payload,
}

pub(crate) struct Source {
    pub(crate) document: Document,
    pub(crate) revision: u64,
    pub(crate) registered: bool,
}
pub(crate) struct Menu {
    pub(crate) token: Ticket,
    pub(crate) owner: Owner,
    pub(crate) source: Document,
}
pub(crate) struct Request {
    pub(crate) menu: Menu,
    pub(crate) deadline: Instant,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Initialization {
    pub(crate) request_id: String,
    pub(crate) update_sequence: u64,
}
pub(crate) struct Preparation {
    pub(crate) target: Document,
    pub(crate) receiver: Receiver,
    pub(crate) initialization: Option<Initialization>,
    pub(crate) prepared: bool,
    pub(crate) confirmed: bool,
    pub(crate) native_prepared: bool,
    pub(crate) step: PrepareStep,
    pub(crate) step_deadline: Instant,
}
pub(crate) struct Cancellation {
    pub(crate) deadline: Instant,
    pub(crate) retain_on_loss: bool,
}
/// Body-free metadata survives body release until this exact document ends.
pub(crate) struct Spent {
    pub(crate) transfer: Ticket,
    pub(crate) target: Document,
    pub(crate) issued: bool,
}
pub(crate) struct Transaction {
    pub(crate) id: Ticket,
    pub(crate) selector: Ticket,
    pub(crate) owner: Owner,
    pub(crate) source: Document,
    pub(crate) revision: u64,
    pub(crate) digest: [u8; 32],
    pub(crate) body: Option<Payload>,
    pub(crate) state: State,
    pub(crate) deadline: Instant,
    pub(crate) preparation: Option<Preparation>,
    pub(crate) ack_eligible: bool,
    pub(crate) document_alive: bool,
    pub(crate) destruction_requested: bool,
    pub(crate) recovery_deadline: Option<Instant>,
    pub(crate) cancellation: Option<Cancellation>,
    pub(crate) input_safe: bool,
}

mod decimal_u64 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub(super) fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        let value = String::deserialize(deserializer)?;
        super::super::payload::revision(&value)
            .map_err(|_| serde::de::Error::custom("invalid identity revision"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SnapshotMetadata {
    pub(crate) content_type: &'static str,
    pub(crate) bytes: usize,
    pub(crate) source_plugin_id: String,
}
/// Post-gate work remains tied to the originating flow, even if a different
/// transfer subsequently uses the same active document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EffectIntent {
    pub(crate) owner: Owner,
    pub(crate) transfer: Ticket,
    pub(crate) effect: Effect,
}
impl EffectIntent {
    pub(super) fn new(transaction: &Transaction, effect: Effect) -> Self {
        Self {
            owner: transaction.owner.clone(),
            transfer: transaction.id.clone(),
            effect,
        }
    }
}
/// Authoritative native state, never supplied by plugin JSON. Revalidate under
/// the shared gate in the main-thread native action immediately before exposure.
pub(crate) struct ExposureView<'a> {
    pub(crate) owner: &'a Owner,
    pub(crate) active_document: &'a Document,
    pub(crate) main_visible: bool,
}
