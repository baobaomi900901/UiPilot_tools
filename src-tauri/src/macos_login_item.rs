//! Main-app login registration only. No startup reconciliation or stored desired bool.
//! Native entry points require the host's serialized AppKit main-thread queue.
use objc2::MainThreadMarker;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Status { NotRegistered, Enabled, RequiresApproval, NotFound, Unavailable }
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Outcome { Applied, Unchanged, RequiresApproval, Unconfirmed }
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct Snapshot { pub(crate) status: Status }
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct Mutation {
    pub(crate) status: Status, pub(crate) outcome: Outcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) issue: Option<RegistrationIssue>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RegistrationIssue { InvalidSignature, PermissionDenied, ServiceUnavailable, NotFound, Unknown }
fn classify_issue(service_domain: bool, code: isize) -> RegistrationIssue {
    if !service_domain { return RegistrationIssue::Unknown; }
    match code {
        3 => RegistrationIssue::InvalidSignature,
        4 | 11 => RegistrationIssue::PermissionDenied,
        5 | 6 | 8 => RegistrationIssue::NotFound,
        7 => RegistrationIssue::ServiceUnavailable,
        _ => RegistrationIssue::Unknown,
    }
}

trait Driver {
    fn status(&mut self) -> Result<isize, ()>;
    fn register(&mut self) -> Result<bool, ()>;
    fn unregister(&mut self) -> Result<bool, ()>;
    fn issue(&self) -> Option<RegistrationIssue> { None }
}
fn decode_status(raw: isize) -> Status {
    match raw { 0 => Status::NotRegistered, 1 => Status::Enabled, 2 => Status::RequiresApproval, 3 => Status::NotFound, _ => Status::Unavailable }
}
fn read_with(driver: &mut impl Driver) -> Snapshot {
    Snapshot { status: driver.status().map(decode_status).unwrap_or(Status::Unavailable) }
}
fn set_with(driver: &mut impl Driver, enabled: bool) -> Mutation {
    let before = read_with(driver).status;
    match (enabled, before) {
        (true, Status::Enabled) | (false, Status::NotRegistered) =>
            return Mutation { status: before, outcome: Outcome::Unchanged, issue: None },
        (true, Status::RequiresApproval) =>
            return Mutation { status: before, outcome: Outcome::RequiresApproval, issue: None },
        (false, Status::NotFound) | (_, Status::Unavailable) =>
            return Mutation { status: before, outcome: Outcome::Unconfirmed, issue: None },
        _ => {}
    }
    let succeeded = if enabled { driver.register() } else { driver.unregister() } == Ok(true);
    // Even a false return or caught Objective-C exception can follow a changed
    // system state. Never substitute the requested bool or an assumed rollback.
    let status = read_with(driver).status;
    let outcome = if !succeeded { Outcome::Unconfirmed }
        else if (enabled && status == Status::Enabled) || (!enabled && status == Status::NotRegistered) { Outcome::Applied }
        else if enabled && status == Status::RequiresApproval { Outcome::RequiresApproval }
        else { Outcome::Unconfirmed };
    Mutation { status, outcome, issue: if succeeded { None } else { driver.issue() } }
}

pub(crate) fn read_native(mtm: MainThreadMarker) -> Snapshot {
    objc2::rc::autoreleasepool(|_| match native::Service::new(mtm) {
        Ok(mut service) => read_with(&mut service),
        Err(()) => Snapshot { status: Status::Unavailable },
    })
}
pub(crate) fn set_native(mtm: MainThreadMarker, enabled: bool) -> Mutation {
    objc2::rc::autoreleasepool(|_| match native::Service::new(mtm) {
        Ok(mut service) => set_with(&mut service, enabled),
        Err(()) => Mutation { status: Status::Unavailable, outcome: Outcome::Unconfirmed, issue: None },
    })
}
pub(crate) fn open_settings_native(mtm: MainThreadMarker) -> Result<(), ()> {
    objc2::rc::autoreleasepool(|_| native::open_settings(mtm))
}

mod native {
    use super::{Driver, RegistrationIssue, classify_issue};
    use objc2::{extern_class, extern_methods, MainThreadMarker, MainThreadOnly, rc::Retained};
    use objc2_foundation::{NSError, NSObject, NSString};
    use std::{panic::AssertUnwindSafe, ptr};

    #[link(name = "ServiceManagement", kind = "framework")]
    unsafe extern "C" { static SMAppServiceErrorDomain: *const NSString; }

    // Minimal binding to the macOS 13+ SDK class. Deliberately constrain this
    // wrapper to AppKit's main thread even though the header uses NSObject.
    // No helper IDs, paths, daemon/agent APIs or error descriptions are exposed.
    extern_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        struct SMAppService;
    );
    impl SMAppService {
        extern_methods!(
            #[unsafe(method(mainAppService))]
            #[unsafe(method_family = none)]
            fn main_service(mtm: MainThreadMarker) -> Option<Retained<Self>>;
            #[unsafe(method(status))]
            #[unsafe(method_family = none)]
            fn service_status(&self) -> isize;
            #[unsafe(method(registerAndReturnError:))]
            #[unsafe(method_family = none)]
            unsafe fn register_service(&self, error: *mut *mut NSError) -> bool;
            #[unsafe(method(unregisterAndReturnError:))]
            #[unsafe(method_family = none)]
            unsafe fn unregister_service(&self, error: *mut *mut NSError) -> bool;
            #[unsafe(method(openSystemSettingsLoginItems))]
            #[unsafe(method_family = none)]
            fn open_settings(mtm: MainThreadMarker);
        );
    }
    pub(super) struct Service(Retained<SMAppService>, Option<RegistrationIssue>);
    impl Service {
        pub(super) fn new(mtm: MainThreadMarker) -> Result<Self, ()> {
            objc2::exception::catch(|| SMAppService::main_service(mtm))
                .map_err(|_| ())?.map(|service| Self(service, None)).ok_or(())
        }
    }
    impl Driver for Service {
        fn status(&mut self) -> Result<isize, ()> {
            objc2::exception::catch(AssertUnwindSafe(|| self.0.service_status())).map_err(|_| ())
        }
        fn register(&mut self) -> Result<bool, ()> { self.change(true) }
        fn unregister(&mut self) -> Result<bool, ()> { self.change(false) }
        fn issue(&self) -> Option<RegistrationIssue> { self.1 }
    }
    impl Service {
        fn change(&mut self, enable: bool) -> Result<bool, ()> {
            self.1 = None;
            let mut error: *mut NSError = ptr::null_mut();
            let result = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
                // NSError is autoreleased into read_native/set_native's pool.
                // No error object or native message crosses the IPC boundary.
                if enable { self.0.register_service(&mut error) }
                else { self.0.unregister_service(&mut error) }
            })).map_err(|_| ());
            if result.is_err() {
                // A thrown call has no reliable NSError out-parameter contract.
                self.1 = Some(RegistrationIssue::Unknown);
            } else if result == Ok(false) {
                self.1 = Some(objc2::exception::catch(AssertUnwindSafe(|| unsafe {
                    match error.as_ref() {
                        Some(error) => classify_issue(SMAppServiceErrorDomain.as_ref()
                            .is_some_and(|domain| &*error.domain() == domain), error.code()),
                        None => RegistrationIssue::Unknown,
                    }
                })).unwrap_or(RegistrationIssue::Unknown));
            }
            result
        }
    }

    pub(super) fn open_settings(mtm: MainThreadMarker) -> Result<(), ()> {
        objc2::exception::catch(|| SMAppService::open_settings(mtm)).map_err(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    struct Fake { statuses: VecDeque<Result<isize, ()>>, mutation: Result<bool, ()>, trace: Vec<&'static str>, issue: Option<RegistrationIssue> }
    impl Fake {
        fn new(statuses: &[Result<isize, ()>], mutation: Result<bool, ()>) -> Self {
            Self { statuses: statuses.iter().copied().collect(), mutation, trace: vec![], issue: None }
        }
    }
    impl Driver for Fake {
        fn issue(&self) -> Option<RegistrationIssue> { self.issue }
        fn status(&mut self) -> Result<isize, ()> { self.trace.push("status"); self.statuses.pop_front().expect("unexpected extra status read") }
        fn register(&mut self) -> Result<bool, ()> { self.trace.push("register"); self.mutation }
        fn unregister(&mut self) -> Result<bool, ()> { self.trace.push("unregister"); self.mutation }
    }
    #[test]
    fn not_found_is_not_rewritten_and_only_explicit_enable_attempts_one_registration() {
        let mut fake = Fake::new(&[Ok(3), Ok(3), Ok(3)], Ok(false));
        assert_eq!(read_with(&mut fake).status, Status::NotFound);
        let result = set_with(&mut fake, true);
        assert_eq!(result.status, Status::NotFound);
        assert_eq!(result.outcome, Outcome::Unconfirmed);
        assert_eq!(fake.trace, ["status", "status", "register", "status"]);
    }
    #[test]
    fn failed_registration_keeps_error_classification_and_postread_fact_independent() {
        let mut fake = Fake::new(&[Ok(3), Ok(1)], Ok(false));
        fake.issue = Some(RegistrationIssue::InvalidSignature);
        let result = set_with(&mut fake, true);
        assert_eq!(result.status, Status::Enabled);
        assert_eq!(result.outcome, Outcome::Unconfirmed);
        assert_eq!(result.issue, Some(RegistrationIssue::InvalidSignature));
        assert_eq!(serde_json::to_value(result).unwrap(), serde_json::json!({"status":"enabled","outcome":"unconfirmed","issue":"invalidSignature"}));
        assert_eq!(fake.trace, ["status", "register", "status"]);
    }
    #[test]
    fn error_classification_requires_service_domain_and_never_exposes_native_text() {
        assert_eq!(classify_issue(false, 3), RegistrationIssue::Unknown);
        for (code, issue) in [(3, RegistrationIssue::InvalidSignature), (11, RegistrationIssue::PermissionDenied), (6, RegistrationIssue::NotFound), (7, RegistrationIssue::ServiceUnavailable), (999, RegistrationIssue::Unknown)] {
            assert_eq!(classify_issue(true, code), issue);
        }
    }
    #[test]
    fn read_maps_all_sdk_statuses_unknown_and_exceptions_without_mutation() {
        for (raw, expected) in [(Ok(0), Status::NotRegistered), (Ok(1), Status::Enabled), (Ok(2), Status::RequiresApproval), (Ok(3), Status::NotFound), (Ok(-1), Status::Unavailable), (Ok(isize::MAX), Status::Unavailable), (Err(()), Status::Unavailable)] {
            let mut fake = Fake::new(&[raw], Ok(true)); assert_eq!(read_with(&mut fake).status, expected);
            assert_eq!(fake.trace, ["status"]);
        }
    }
    #[test]
    fn no_write_transition_table_respects_system_approval_and_unavailable_states() {
        for (initial, enabled, outcome) in [
            (0, false, Outcome::Unchanged), (1, true, Outcome::Unchanged),
            (2, true, Outcome::RequiresApproval),
            (3, false, Outcome::Unconfirmed), (99, true, Outcome::Unconfirmed), (99, false, Outcome::Unconfirmed),
        ] {
            let mut fake = Fake::new(&[Ok(initial)], Ok(true));
            assert_eq!(set_with(&mut fake, enabled), Mutation { status: decode_status(initial), outcome, issue: None });
            assert_eq!(fake.trace, ["status"], "state={initial} enable={enabled}");
        }
    }
    #[test]
    fn every_mutating_transition_rereads_and_requires_success_plus_target_fact() {
        let afters = [Ok(0), Ok(1), Ok(2), Ok(3), Ok(99), Err(())];
        for (initial, enabled, operation) in [(0, true, "register"), (3, true, "register"), (1, false, "unregister"), (2, false, "unregister")] {
            for returned in [Ok(true), Ok(false), Err(())] {
                for after in afters {
                    let status = after.map(decode_status).unwrap_or(Status::Unavailable);
                    let outcome = if returned != Ok(true) { Outcome::Unconfirmed }
                        else if (enabled && status == Status::Enabled) || (!enabled && status == Status::NotRegistered) { Outcome::Applied }
                        else if enabled && status == Status::RequiresApproval { Outcome::RequiresApproval }
                        else { Outcome::Unconfirmed };
                    let mut fake = Fake::new(&[Ok(initial), after], returned);
                    assert_eq!(set_with(&mut fake, enabled), Mutation { status, outcome, issue: None }, "initial={initial}, enable={enabled}, return={returned:?}, after={after:?}");
                    assert_eq!(fake.trace, ["status", operation, "status"]);
                    assert!(fake.statuses.is_empty());
                }
            }
        }
    }
    #[test]
    fn external_revocation_is_visible_on_refresh_and_does_not_trigger_registration() {
        let mut fake = Fake::new(&[Ok(1), Ok(2), Ok(2)], Ok(true));
        assert_eq!(read_with(&mut fake).status, Status::Enabled);
        assert_eq!(read_with(&mut fake).status, Status::RequiresApproval);
        assert_eq!(set_with(&mut fake, true).outcome, Outcome::RequiresApproval);
        assert_eq!(fake.trace, ["status", "status", "status"]);
    }
    #[test]
    fn failed_change_can_report_enabled_fact_without_claiming_applied_or_rollback() {
        for returned in [Ok(false), Err(())] {
            let mut fake = Fake::new(&[Ok(0), Ok(1)], returned);
            assert_eq!(set_with(&mut fake, true), Mutation { status: Status::Enabled, outcome: Outcome::Unconfirmed, issue: None });
            assert_eq!(fake.trace, ["status", "register", "status"]);
        }
    }
    #[test]
    fn initial_status_exception_is_unavailable_and_does_not_attempt_changes() {
        for enabled in [true, false] {
            let mut fake = Fake::new(&[Err(())], Ok(true));
            assert_eq!(set_with(&mut fake, enabled), Mutation { status: Status::Unavailable, outcome: Outcome::Unconfirmed, issue: None });
            assert_eq!(fake.trace, ["status"]);
        }
    }
    #[test]
    fn wire_format_is_only_fixed_status_and_outcome_values() {
        assert_eq!(serde_json::to_value(Snapshot { status: Status::NotRegistered }).unwrap(), serde_json::json!({"status":"notRegistered"}));
        assert_eq!(serde_json::to_value(Mutation { status: Status::RequiresApproval, outcome: Outcome::RequiresApproval, issue: None }).unwrap(), serde_json::json!({"status":"requiresApproval","outcome":"requiresApproval"}));
        assert_eq!(serde_json::to_value(Mutation { status: Status::Unavailable, outcome: Outcome::Unconfirmed, issue: None }).unwrap(), serde_json::json!({"status":"unavailable","outcome":"unconfirmed"}));
    }
}
