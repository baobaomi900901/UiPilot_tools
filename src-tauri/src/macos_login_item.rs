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
pub(crate) struct Mutation { pub(crate) status: Status, pub(crate) outcome: Outcome }

trait Driver {
    fn status(&mut self) -> Result<isize, ()>;
    fn register(&mut self) -> Result<bool, ()>;
    fn unregister(&mut self) -> Result<bool, ()>;
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
            return Mutation { status: before, outcome: Outcome::Unchanged },
        (true, Status::RequiresApproval) =>
            return Mutation { status: before, outcome: Outcome::RequiresApproval },
        (_, Status::NotFound | Status::Unavailable) =>
            return Mutation { status: before, outcome: Outcome::Unconfirmed },
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
    Mutation { status, outcome }
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
        Err(()) => Mutation { status: Status::Unavailable, outcome: Outcome::Unconfirmed },
    })
}
pub(crate) fn open_settings_native(mtm: MainThreadMarker) -> Result<(), ()> {
    objc2::rc::autoreleasepool(|_| native::open_settings(mtm))
}

mod native {
    use super::Driver;
    use objc2::{extern_class, extern_methods, MainThreadMarker, MainThreadOnly, rc::Retained};
    use objc2_foundation::{NSError, NSObject};
    use std::{panic::AssertUnwindSafe, ptr};

    #[link(name = "ServiceManagement", kind = "framework")]
    unsafe extern "C" {}

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
    pub(super) struct Service(Retained<SMAppService>);
    impl Service {
        pub(super) fn new(mtm: MainThreadMarker) -> Result<Self, ()> {
            objc2::exception::catch(|| SMAppService::main_service(mtm))
                .map_err(|_| ())?.map(Self).ok_or(())
        }
    }
    impl Driver for Service {
        fn status(&mut self) -> Result<isize, ()> {
            objc2::exception::catch(AssertUnwindSafe(|| self.0.service_status())).map_err(|_| ())
        }
        fn register(&mut self) -> Result<bool, ()> {
            // SAFETY: SDK explicitly permits NULL NSError**. The retained main
            // service and its thread-only receiver remain alive for this call.
            objc2::exception::catch(AssertUnwindSafe(|| unsafe { self.0.register_service(ptr::null_mut()) })).map_err(|_| ())
        }
        fn unregister(&mut self) -> Result<bool, ()> {
            // SAFETY: same nullable NSError** contract as registerAndReturnError:.
            objc2::exception::catch(AssertUnwindSafe(|| unsafe { self.0.unregister_service(ptr::null_mut()) })).map_err(|_| ())
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
    struct Fake { statuses: VecDeque<Result<isize, ()>>, mutation: Result<bool, ()>, trace: Vec<&'static str> }
    impl Fake {
        fn new(statuses: &[Result<isize, ()>], mutation: Result<bool, ()>) -> Self {
            Self { statuses: statuses.iter().copied().collect(), mutation, trace: vec![] }
        }
    }
    impl Driver for Fake {
        fn status(&mut self) -> Result<isize, ()> { self.trace.push("status"); self.statuses.pop_front().expect("unexpected extra status read") }
        fn register(&mut self) -> Result<bool, ()> { self.trace.push("register"); self.mutation }
        fn unregister(&mut self) -> Result<bool, ()> { self.trace.push("unregister"); self.mutation }
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
            (2, true, Outcome::RequiresApproval), (3, true, Outcome::Unconfirmed),
            (3, false, Outcome::Unconfirmed), (99, true, Outcome::Unconfirmed), (99, false, Outcome::Unconfirmed),
        ] {
            let mut fake = Fake::new(&[Ok(initial)], Ok(true));
            assert_eq!(set_with(&mut fake, enabled), Mutation { status: decode_status(initial), outcome });
            assert_eq!(fake.trace, ["status"], "state={initial} enable={enabled}");
        }
    }
    #[test]
    fn every_mutating_transition_rereads_and_requires_success_plus_target_fact() {
        let afters = [Ok(0), Ok(1), Ok(2), Ok(3), Ok(99), Err(())];
        for (initial, enabled, operation) in [(0, true, "register"), (1, false, "unregister"), (2, false, "unregister")] {
            for returned in [Ok(true), Ok(false), Err(())] {
                for after in afters {
                    let status = after.map(decode_status).unwrap_or(Status::Unavailable);
                    let outcome = if returned != Ok(true) { Outcome::Unconfirmed }
                        else if (enabled && status == Status::Enabled) || (!enabled && status == Status::NotRegistered) { Outcome::Applied }
                        else if enabled && status == Status::RequiresApproval { Outcome::RequiresApproval }
                        else { Outcome::Unconfirmed };
                    let mut fake = Fake::new(&[Ok(initial), after], returned);
                    assert_eq!(set_with(&mut fake, enabled), Mutation { status, outcome }, "initial={initial}, enable={enabled}, return={returned:?}, after={after:?}");
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
            assert_eq!(set_with(&mut fake, true), Mutation { status: Status::Enabled, outcome: Outcome::Unconfirmed });
            assert_eq!(fake.trace, ["status", "register", "status"]);
        }
    }
    #[test]
    fn initial_status_exception_is_unavailable_and_does_not_attempt_changes() {
        for enabled in [true, false] {
            let mut fake = Fake::new(&[Err(())], Ok(true));
            assert_eq!(set_with(&mut fake, enabled), Mutation { status: Status::Unavailable, outcome: Outcome::Unconfirmed });
            assert_eq!(fake.trace, ["status"]);
        }
    }
    #[test]
    fn wire_format_is_only_fixed_status_and_outcome_values() {
        assert_eq!(serde_json::to_value(Snapshot { status: Status::NotRegistered }).unwrap(), serde_json::json!({"status":"notRegistered"}));
        assert_eq!(serde_json::to_value(Mutation { status: Status::RequiresApproval, outcome: Outcome::RequiresApproval }).unwrap(), serde_json::json!({"status":"requiresApproval","outcome":"requiresApproval"}));
        assert_eq!(serde_json::to_value(Mutation { status: Status::Unavailable, outcome: Outcome::Unconfirmed }).unwrap(), serde_json::json!({"status":"unavailable","outcome":"unconfirmed"}));
    }
}
