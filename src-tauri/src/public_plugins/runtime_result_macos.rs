//! Fixed legacy MainResult completion path. Actions await actual permission and
//! execution wiring; this internal path accepts informational results only.
use super::{
    call_session::{CallSessionError, CompletionTicket},
    main_result_contract::{parse_main_result_response, PublicMainResult, MAX_MAIN_RESPONSE_BYTES},
    runtime_data_macos::RuntimeDataBinding,
    storage_document::validate_mac_json,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MainCompletionError {
    InvalidResult,
    UnsupportedAction,
    Expired,
    Busy,
    Unavailable,
}
type Result<T> = std::result::Result<T, MainCompletionError>;

/// No public constructor or Clone: transport retains this until normal terminal
/// evidence is known. Its session ticket is unforgeable and single-use.
pub(crate) struct PreparedMainResults {
    ticket: CompletionTicket,
    rows: Vec<PublicMainResult>,
}
impl RuntimeDataBinding {
    pub(crate) fn prepare_main_results(&self, raw: &[u8]) -> Result<PreparedMainResults> {
        if raw.len() > MAX_MAIN_RESPONSE_BYTES || validate_mac_json(raw).is_none() {
            return Err(MainCompletionError::InvalidResult);
        }
        let value = serde_json::from_slice(raw).map_err(|_| MainCompletionError::InvalidResult)?;
        let rows = parse_main_result_response(self.context(), value)
            .map_err(|_| MainCompletionError::InvalidResult)?;
        if rows.iter().any(|row| row.copy_text.is_some()) {
            return Err(MainCompletionError::UnsupportedAction);
        }
        let ticket = self
            .session()
            .prepare_completion(self.endpoint(), self.context(), &self.context().request_id)
            .map_err(map_session)?;
        Ok(PreparedMainResults { ticket, rows })
    }

    /// Adapter must verify transport terminal evidence BEFORE this call. The
    /// sink must check its final owner and commit synchronously, never queue an
    /// unchecked future UI write from this closure.
    pub(crate) fn submit_main_results<T>(
        &self,
        prepared: PreparedMainResults,
        publish: impl FnOnce(Vec<PublicMainResult>) -> T,
    ) -> Result<T> {
        self.session()
            .submit_completion(self.endpoint(), prepared.ticket, || publish(prepared.rows))
            .map_err(map_session)
    }
}
fn map_session(error: CallSessionError) -> MainCompletionError {
    match error {
        CallSessionError::Busy => MainCompletionError::Busy,
        CallSessionError::Unavailable => MainCompletionError::Unavailable,
        _ => MainCompletionError::Expired,
    }
}

#[cfg(test)]
#[path = "runtime_result_macos_tests.rs"]
pub(super) mod tests;
