//! Explicit one-shot copy only. Never a temporary backup/restore transaction.
//! Count-check -> declare is NOT a cross-process compare-and-swap on macOS.
use std::{panic::AssertUnwindSafe, sync::Arc, thread::{self, JoinHandle}};
use objc2::rc::{autoreleasepool, Retained};
use objc2_app_kit::NSPasteboard;
use objc2_foundation::{NSArray, NSData, NSString};
use super::{macos_history::{CopyToken, MacHistory}, macos_observer::ChangeCount, macos_store::MAX_TEXT};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CopyPurpose { ExplicitCopy, TemporaryRestore }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CopyError {
    Unsupported, PermissionDenied, RecordNotFound, RecordUnavailable,
    Unavailable, ClipboardChanged, OwnershipLost, WriteUnconfirmed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CopyOutcome {
    /// setData accepted this write. Another application may already have replaced it.
    Accepted,
}
/// Non-Clone intent. No plaintext getter, store, file descriptor or prior clipboard.
pub(super) struct CopyRequest { token: CopyToken, id: String }
fn check_purpose(purpose: CopyPurpose) -> Result<(), CopyError> {
    match purpose { CopyPurpose::ExplicitCopy => Ok(()), CopyPurpose::TemporaryRestore => Err(CopyError::Unsupported) }
}
pub(super) fn prepare_request(history: &MacHistory, token: &CopyToken, id: &str, purpose: CopyPurpose) -> Result<CopyRequest, CopyError> {
    check_purpose(purpose)?;
    history.with_copy_record(token, id, |_| Ok(()))?;
    Ok(CopyRequest { token: token.clone(), id: id.into() })
}

trait CopyDriver {
    type Prepared;
    fn change_count(&mut self) -> Result<ChangeCount, CopyError>;
    fn prepare(&mut self, text: &str) -> Result<Self::Prepared, CopyError>;
    /// Exactly one ownership declaration and one set attempt. Never retries or restores.
    fn publish(&mut self, prepared: Self::Prepared) -> Result<(), CopyError>;
}

fn execute_with<D: CopyDriver>(history: &MacHistory, request: CopyRequest,
    factory: impl FnOnce() -> Result<D, CopyError>) -> Result<CopyOutcome, CopyError> {
    // Reject stale authority or missing records before constructing a native driver.
    let text = history.with_copy_record(&request.token, &request.id, |text| Ok(text.to_owned()))?;
    if text.len() > MAX_TEXT { return Err(CopyError::RecordUnavailable); }
    let mut driver = factory()?;
    let baseline = driver.change_count()?;
    let prepared = driver.prepare(&text)?;
    history.with_copy_record(&request.token, &request.id, |current| {
        if current != text { return Err(CopyError::RecordUnavailable); }
        if driver.change_count()? != baseline { return Err(CopyError::ClipboardChanged); }
        // Not CAS: an external write after this check but before declaration may
        // be replaced by this explicit copy. No restoration is offered.
        driver.publish(prepared)?;
        Ok(CopyOutcome::Accepted)
    })
}

/// Unassembled native entry point. Native objects live entirely in this worker.
/// The caller must still validate future UI/session ownership at final admission.
pub(super) fn copy_to_general(history: Arc<MacHistory>, request: CopyRequest)
    -> Result<JoinHandle<Result<CopyOutcome, CopyError>>, CopyError> {
    thread::Builder::new().name("uipilot-macos-copy".into()).spawn(move || autoreleasepool(|_| {
        execute_with(&history, request, || {
            objc2::exception::catch(|| NativeDriver(NSPasteboard::generalPasteboard())).map_err(|_| CopyError::Unavailable)
        })
    })).map_err(|_| CopyError::Unavailable)
}

struct NativeDriver(Retained<NSPasteboard>);
struct NativePrepared { data: Retained<NSData>, kind: Retained<NSString>, types: Retained<NSArray<NSString>> }
impl CopyDriver for NativeDriver {
    type Prepared = NativePrepared;
    fn change_count(&mut self) -> Result<ChangeCount, CopyError> {
        objc2::exception::catch(AssertUnwindSafe(|| ChangeCount(self.0.changeCount()))).map_err(|_| CopyError::Unavailable)
    }
    fn prepare(&mut self, text: &str) -> Result<Self::Prepared, CopyError> {
        objc2::exception::catch(|| {
            let kind = NSString::from_str("public.utf8-plain-text");
            let types = NSArray::from_slice(&[&*kind]);
            NativePrepared { data: NSData::with_bytes(text.as_bytes()), kind, types }
        }).map_err(|_| CopyError::Unavailable)
    }
    fn publish(&mut self, prepared: Self::Prepared) -> Result<(), CopyError> {
        // Mutation is considered started BEFORE declare, including exceptions
        // thrown by declare itself. Catch objects are discarded without formatting.
        objc2::exception::catch(AssertUnwindSafe(|| {
            // SAFETY: nil owner is documented for immediately supplied data; all
            // type objects and bytes are prepared and retained before mutation.
            unsafe { self.0.declareTypes_owner(&prepared.types, None); }
            if self.0.setData_forType(Some(&prepared.data), &prepared.kind) {
                Ok(())
            } else { Err(CopyError::OwnershipLost) }
        })).map_err(|_| CopyError::WriteUnconfirmed)?
    }
}

#[cfg(test)]
#[path = "macos_copy_tests.rs"]
mod tests;
