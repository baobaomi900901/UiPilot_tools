use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    path::PathBuf,
};

use super::*;
use crate::process_control::batch::{
    ProcessBatchError, ProcessBatchOperation, ProcessBatchPlan, ProcessBatchPreview,
    ProcessBatchScope, ProcessBatchSelectionCompleteness,
};
use crate::process_control::core::ExpansionOmissionReason;

mod plan_is_not_clone {
    use super::ProcessBatchPlan;

    trait AmbiguousIfImpl<A> {
        fn marker() {}
    }
    impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
    struct Implemented;
    impl<T: ?Sized + Clone> AmbiguousIfImpl<Implemented> for T {}

    pub(super) fn assert_contract() {
        let _ = <ProcessBatchPlan as AmbiguousIfImpl<_>>::marker;
    }
}

mod plan_is_not_serialize {
    use super::ProcessBatchPlan;

    trait AmbiguousIfImpl<A> {
        fn marker() {}
    }
    impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
    struct Implemented;
    impl<T: ?Sized + serde::Serialize> AmbiguousIfImpl<Implemented> for T {}

    pub(super) fn assert_contract() {
        let _ = <ProcessBatchPlan as AmbiguousIfImpl<_>>::marker;
    }
}

mod preview_is_not_clone {
    use super::ProcessBatchPreview;

    trait AmbiguousIfImpl<A> {
        fn marker() {}
    }
    impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
    struct Implemented;
    impl<T: ?Sized + Clone> AmbiguousIfImpl<Implemented> for T {}

    pub(super) fn assert_contract() {
        let _ = <ProcessBatchPreview as AmbiguousIfImpl<_>>::marker;
    }
}

mod preview_is_not_serialize {
    use super::ProcessBatchPreview;

    trait AmbiguousIfImpl<A> {
        fn marker() {}
    }
    impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
    struct Implemented;
    impl<T: ?Sized + serde::Serialize> AmbiguousIfImpl<Implemented> for T {}

    pub(super) fn assert_contract() {
        let _ = <ProcessBatchPreview as AmbiguousIfImpl<_>>::marker;
    }
}

fn planning_script(creation_time: u64, name: &str, path: &str) -> FakeProcessScript {
    FakeProcessScript {
        open: vec![Ok(()), Ok(())].into(),
        creation_times: vec![Ok(creation_time), Ok(creation_time)].into(),
        liveness: vec![Ok(Liveness::Alive), Ok(Liveness::Alive)].into(),
        observations: vec![Ok(path_observation(name, path))].into(),
        protections: vec![Ok(ProcessProtection::new(false, false))].into(),
        ..FakeProcessScript::default()
    }
}

fn absent_root_script(
    creation_time: u64,
    repoll: Result<Liveness, ObservationFailure>,
) -> FakeProcessScript {
    FakeProcessScript {
        open: vec![Ok(())].into(),
        creation_times: vec![Ok(creation_time)].into(),
        liveness: vec![Ok(Liveness::Alive), repoll].into(),
        protections: vec![Ok(ProcessProtection::new(false, false))].into(),
        ..FakeProcessScript::default()
    }
}

#[test]
fn batch_catalog_contract_is_opaque_redacted_and_side_effect_free() {
    plan_is_not_clone::assert_contract();
    plan_is_not_serialize::assert_contract();
    preview_is_not_clone::assert_contract();
    preview_is_not_serialize::assert_contract();

    let errors = [
        ProcessBatchError::InvalidTarget,
        ProcessBatchError::ExpiredTarget,
        ProcessBatchError::TargetExited,
        ProcessBatchError::TargetChanged,
        ProcessBatchError::InspectionDenied,
        ProcessBatchError::ProtectedTarget,
        ProcessBatchError::CatalogUnavailable,
        ProcessBatchError::TargetSetTooLarge,
        ProcessBatchError::ExpiredPlan,
        ProcessBatchError::PlatformUnsupported,
    ];
    assert_eq!(
        errors.map(|error| error.to_string()),
        [
            "InvalidTarget",
            "ExpiredTarget",
            "TargetExited",
            "TargetChanged",
            "InspectionDenied",
            "ProtectedTarget",
            "CatalogUnavailable",
            "TargetSetTooLarge",
            "ExpiredPlan",
            "PlatformUnsupported",
        ]
    );
    assert!(errors
        .iter()
        .all(|error| !error.to_string().contains("sensitive-fixture.exe")));

    let batch_source = include_str!("../batch.rs");
    let module_source = include_str!("../mod.rs");
    let plan_fields = batch_source
        .split("pub(crate) struct ProcessBatchPlan")
        .nth(1)
        .expect("ProcessBatchPlan must be declared")
        .split("impl ProcessBatchPlan")
        .next()
        .expect("ProcessBatchPlan accessors must follow the fields");
    let plan_accessors = batch_source
        .split("impl ProcessBatchPlan")
        .nth(1)
        .expect("ProcessBatchPlan accessors must be declared")
        .split("pub(crate) struct ProcessBatchPreview")
        .next()
        .expect("ProcessBatchPreview must follow plan accessors");
    let preview_fields = batch_source
        .split("pub(crate) struct ProcessBatchPreview")
        .nth(1)
        .expect("ProcessBatchPreview must be declared")
        .split("impl ProcessBatchPreview")
        .next()
        .expect("ProcessBatchPreview accessors must follow the fields");

    assert!(plan_fields.contains("targets: Vec<ProcessTarget>"));
    assert!(!plan_accessors.contains("targets("));
    for forbidden in ["Clone", "Serialize", "Deserialize"] {
        assert!(
            !plan_fields.contains(forbidden),
            "plan contains {forbidden}"
        );
        assert!(
            !preview_fields.contains(forbidden),
            "preview contains {forbidden}"
        );
    }
    for forbidden in [
        "ProcessTarget",
        "creation_time",
        "HostInstanceId",
        "HANDLE",
        "raw_",
        "candidate",
    ] {
        assert!(
            !preview_fields.contains(forbidden),
            "preview leaked {forbidden}"
        );
    }
    for forbidden in [
        "OpenProcess",
        "CreateToolhelp32Snapshot",
        "TerminateProcess",
        "taskkill",
        "PowerShell",
        "Command::new",
        "ShellExecute",
        "tauri",
        "tracing::",
        "log::",
        "std::fs",
    ] {
        assert!(
            !batch_source.contains(forbidden),
            "found forbidden {forbidden}"
        );
    }
    assert!(!module_source.contains("ExpansionSnapshot"));
    assert!(!module_source.contains("ExpansionEntry"));
    assert!(!module_source.contains("ExpansionOmissionReason"));

    let _ = ProcessBatchScope::Tree;
    let _ = ProcessBatchScope::SameName;
    let _ = ProcessBatchOperation::Terminate;
    let _ = ProcessBatchOperation::ForceTerminate;
    let _ = ProcessBatchSelectionCompleteness::CompleteSelection;
    let _ = ProcessBatchSelectionCompleteness::IncompleteSelection;
}

#[test]
fn batch_catalog_preflight_rejects_invalid_and_expired_roots_before_platform_calls() {
    for invalid in [
        ProcessTarget::from_parts_for_test([0; 16], 501, 50_100),
        ProcessTarget::from_parts_for_test(HOST, 0, 50_100),
        ProcessTarget::from_parts_for_test(HOST, 501, 0),
    ] {
        let adapter = FakeProcessAdapter::default();
        assert_eq!(
            authority(adapter.clone())
                .prepare_batch_planning(&invalid, ProcessBatchOperation::Terminate)
                .err(),
            Some(ProcessBatchError::InvalidTarget)
        );
        assert!(adapter.calls().is_empty());
    }

    let adapter = FakeProcessAdapter::default();
    let expired =
        ProcessTarget::new(HostInstanceId::new([0x52; 16]).unwrap(), 502, 50_200).unwrap();
    assert_eq!(
        authority(adapter.clone())
            .prepare_batch_planning(&expired, ProcessBatchOperation::ForceTerminate)
            .err(),
        Some(ProcessBatchError::ExpiredTarget)
    );
    assert!(adapter.calls().is_empty());
    assert_eq!(adapter.catalog_opened(), 0);
}

#[test]
fn batch_catalog_preflight_preserves_stage_one_error_priority() {
    let cases = [
        (
            503,
            50_300,
            admission_script(
                50_300,
                Ok(Liveness::Exited),
                Ok(ProcessProtection::new(true, true)),
            ),
            ProcessBatchError::TargetExited,
        ),
        (
            504,
            50_400,
            admission_script(
                50_401,
                Ok(Liveness::Exited),
                Ok(ProcessProtection::new(true, true)),
            ),
            ProcessBatchError::TargetChanged,
        ),
        (
            505,
            50_500,
            FakeProcessScript {
                open: vec![Err(ObservationFailure::AccessDenied)].into(),
                ..FakeProcessScript::default()
            },
            ProcessBatchError::InspectionDenied,
        ),
        (
            506,
            50_600,
            admission_script(
                50_600,
                Ok(Liveness::Alive),
                Ok(ProcessProtection::new(false, true)),
            ),
            ProcessBatchError::ProtectedTarget,
        ),
    ];

    for (pid, creation_time, script, expected) in cases {
        let adapter = FakeProcessAdapter::default().with_process(pid, script);
        assert_eq!(
            authority(adapter.clone())
                .prepare_batch_planning(
                    &target(pid, creation_time),
                    ProcessBatchOperation::Terminate,
                )
                .err(),
            Some(expected)
        );
        assert_eq!(adapter.catalog_opened(), 0);
        assert_eq!(adapter.opened(), adapter.dropped());
    }
}

#[test]
fn batch_catalog_creation_and_traversal_failures_are_atomic() {
    let pid = 507;
    for adapter in [
        FakeProcessAdapter::default()
            .with_enumeration_failure()
            .with_process(
                pid,
                admission_script(
                    50_700,
                    Ok(Liveness::Alive),
                    Ok(ProcessProtection::new(false, false)),
                ),
            ),
        FakeProcessAdapter::default()
            .with_enumeration(vec![
                Ok(candidate(pid, "root.exe", None)),
                Err(EnumerationFailure::Unavailable),
            ])
            .with_process(
                pid,
                admission_script(
                    50_700,
                    Ok(Liveness::Alive),
                    Ok(ProcessProtection::new(false, false)),
                ),
            ),
    ] {
        assert_eq!(
            authority(adapter.clone())
                .prepare_batch_planning(&target(pid, 50_700), ProcessBatchOperation::Terminate,)
                .err(),
            Some(ProcessBatchError::CatalogUnavailable)
        );
        assert_eq!(adapter.catalog_opened(), 1);
        assert_eq!(adapter.catalog_dropped(), 1);
        assert!(!adapter.catalog_active());
        assert_eq!(adapter.opened(), 1);
        assert_eq!(adapter.dropped(), 1);
    }
}

#[test]
fn batch_catalog_prepared_context_is_handle_free_after_one_snapshot() {
    let root_pid = 508;
    let child_pid = 509;
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![
            Ok(candidate(root_pid, "root.exe", None)),
            Ok(candidate(child_pid, "child.exe", Some(root_pid))),
        ])
        .with_process(
            root_pid,
            planning_script(50_800, "root.exe", r"C:\Fixtures\root.exe"),
        )
        .with_process(
            child_pid,
            successful_script(50_900, "child.exe", r"C:\Fixtures\child.exe"),
        );

    let prepared = authority(adapter.clone())
        .prepare_batch_planning(
            &target(root_pid, 50_800),
            ProcessBatchOperation::ForceTerminate,
        )
        .unwrap();

    assert_eq!(prepared.authority_id().as_bytes(), HOST);
    assert_eq!(prepared.operation(), ProcessBatchOperation::ForceTerminate);
    assert_eq!(prepared.root().pid(), root_pid);
    assert_eq!(prepared.snapshot().entries().len(), 2);
    assert_eq!(prepared.snapshot().entries()[1].pid_hint(), child_pid);
    assert_eq!(prepared.snapshot().entries()[1].name_hint(), "child.exe");
    assert_eq!(
        prepared.snapshot().entries()[1].observed_parent_pid_hint(),
        Some(root_pid)
    );
    assert_eq!(
        prepared.snapshot().entries()[1]
            .record()
            .and_then(|record| record.executable_path()),
        Some(PathBuf::from(r"C:\Fixtures\child.exe").as_path())
    );
    assert_eq!(adapter.catalog_opened(), 1);
    assert_eq!(adapter.catalog_dropped(), 1);
    assert!(!adapter.catalog_active());
    assert_eq!(adapter.opened(), 3);
    assert_eq!(adapter.dropped(), 3);

    let enumerate_event = adapter
        .events()
        .into_iter()
        .find(|event| {
            matches!(
                event,
                FakeEvent::Call {
                    call: FakeCall::Enumerate,
                    ..
                }
            )
        })
        .expect("enumeration must be observed");
    let FakeEvent::Call { active_handles, .. } = enumerate_event else {
        unreachable!()
    };
    assert_eq!(active_handles.len(), 1, "root admission must stay open");
}

#[test]
fn batch_catalog_same_pid_replacement_wins_over_absent_root_repoll() {
    let pid = 510;
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![Ok(candidate(pid, "replacement.exe", None))])
        .with_process(pid, absent_root_script(51_000, Ok(Liveness::Exited)))
        .mutate_after(
            FakeCall::Enumerate,
            1,
            FakeMutation::ReplaceProcess {
                pid,
                script: successful_script(
                    51_001,
                    "replacement.exe",
                    r"C:\Fixtures\replacement.exe",
                ),
            },
        );

    assert_eq!(
        authority(adapter.clone())
            .prepare_batch_planning(&target(pid, 51_000), ProcessBatchOperation::Terminate,)
            .err(),
        Some(ProcessBatchError::TargetChanged)
    );
    assert_eq!(
        adapter
            .calls()
            .iter()
            .filter(|call| **call == FakeCall::Liveness(pid))
            .count(),
        2,
        "one root admission poll plus one replacement observation poll"
    );
    assert_eq!(adapter.opened(), adapter.dropped());
}

#[test]
fn batch_catalog_absent_root_repoll_classifies_exit_inspection_and_missing_catalog() {
    for (offset, repoll, expected) in [
        (0, Ok(Liveness::Exited), ProcessBatchError::TargetExited),
        (
            1,
            Err(ObservationFailure::AccessDenied),
            ProcessBatchError::InspectionDenied,
        ),
        (
            2,
            Ok(Liveness::Alive),
            ProcessBatchError::CatalogUnavailable,
        ),
    ] {
        let pid = 511 + offset;
        let creation_time = 51_100 + offset as u64;
        let adapter = FakeProcessAdapter::default()
            .with_enumeration(vec![])
            .with_process(pid, absent_root_script(creation_time, repoll));

        assert_eq!(
            authority(adapter.clone())
                .prepare_batch_planning(
                    &target(pid, creation_time),
                    ProcessBatchOperation::Terminate,
                )
                .err(),
            Some(expected)
        );
        assert_eq!(adapter.catalog_opened(), 1);
        assert_eq!(adapter.catalog_dropped(), 1);
        assert_eq!(adapter.opened(), 1);
        assert_eq!(adapter.dropped(), 1);
    }
}

#[test]
fn batch_catalog_unreliable_root_candidate_is_inspection_denied_without_repoll() {
    let pid = 514;
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![Ok(candidate(pid, "root.exe", None))])
        .with_process(
            pid,
            FakeProcessScript {
                open: vec![Ok(()), Err(ObservationFailure::AccessDenied)].into(),
                creation_times: vec![Ok(51_400)].into(),
                liveness: vec![Ok(Liveness::Alive)].into(),
                protections: vec![Ok(ProcessProtection::new(false, false))].into(),
                ..FakeProcessScript::default()
            },
        );

    assert_eq!(
        authority(adapter.clone())
            .prepare_batch_planning(&target(pid, 51_400), ProcessBatchOperation::Terminate,)
            .err(),
        Some(ProcessBatchError::InspectionDenied)
    );
    assert_eq!(
        adapter.calls(),
        vec![
            FakeCall::OpenObservation(pid),
            FakeCall::CreationTime(pid),
            FakeCall::Liveness(pid),
            FakeCall::Protection(pid),
            FakeCall::Enumerate,
            FakeCall::OpenObservation(pid),
        ]
    );
    assert_eq!(adapter.opened(), adapter.dropped());
}

#[test]
fn batch_catalog_duplicate_pid_invalidates_all_expansion_entries_only() {
    let root_pid = 515;
    let duplicate_pid = 516;
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![
            Ok(candidate(root_pid, "root.exe", None)),
            Ok(candidate(duplicate_pid, "first.exe", Some(root_pid))),
            Ok(candidate(duplicate_pid, "second.exe", Some(root_pid))),
        ])
        .with_enumeration(vec![
            Ok(candidate(duplicate_pid, "first.exe", Some(root_pid))),
            Ok(candidate(duplicate_pid, "second.exe", Some(root_pid))),
        ])
        .with_process(
            root_pid,
            planning_script(51_500, "root.exe", r"C:\Fixtures\root.exe"),
        )
        .with_process(
            duplicate_pid,
            successful_script(51_600, "first.exe", r"C:\Fixtures\first.exe"),
        );
    let authority = authority(adapter.clone());

    let prepared = authority
        .prepare_batch_planning(&target(root_pid, 51_500), ProcessBatchOperation::Terminate)
        .unwrap();
    let duplicates = &prepared.snapshot().entries()[1..];
    assert!(duplicates.iter().all(|entry| entry.record().is_none()));
    assert!(duplicates.iter().all(|entry| {
        entry.omission_reason() == Some(ExpansionOmissionReason::ExitedOrChanged)
    }));
    assert_eq!(
        adapter
            .calls()
            .iter()
            .filter(|call| **call == FakeCall::OpenObservation(duplicate_pid))
            .count(),
        0
    );

    let ordinary = authority.snapshot().unwrap();
    assert_eq!(ordinary.processes().len(), 1);
    assert_eq!(ordinary.processes()[0].pid(), duplicate_pid);
    assert_eq!(ordinary.omissions().exited_or_changed(), 1);
    assert_eq!(adapter.opened(), adapter.dropped());
}

#[test]
fn batch_catalog_invalid_and_failed_candidates_keep_private_omission_reasons() {
    let root_pid = 517;
    let denied_pid = 518;
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![
            Ok(candidate(root_pid, "root.exe", None)),
            Ok(candidate(0, "zero.exe", Some(root_pid))),
            Ok(candidate(519, "", Some(root_pid))),
            Ok(candidate(denied_pid, "private-secret.exe", Some(root_pid))),
        ])
        .with_process(
            root_pid,
            planning_script(51_700, "root.exe", r"C:\Fixtures\root.exe"),
        )
        .with_process(
            denied_pid,
            FakeProcessScript {
                open: vec![Err(ObservationFailure::AccessDenied)].into(),
                ..FakeProcessScript::default()
            },
        );

    let prepared = authority(adapter.clone())
        .prepare_batch_planning(&target(root_pid, 51_700), ProcessBatchOperation::Terminate)
        .unwrap();
    let entries = prepared.snapshot().entries();
    assert_eq!(
        entries[1].omission_reason(),
        Some(ExpansionOmissionReason::IdentityUnavailable)
    );
    assert_eq!(entries[1].pid_hint(), 0);
    assert_eq!(entries[1].name_hint(), "zero.exe");
    assert_eq!(
        entries[2].omission_reason(),
        Some(ExpansionOmissionReason::IdentityUnavailable)
    );
    assert_eq!(entries[2].name_hint(), "");
    assert_eq!(
        entries[3].omission_reason(),
        Some(ExpansionOmissionReason::InspectionDenied)
    );
    assert_eq!(entries[3].name_hint(), "private-secret.exe");
    assert_eq!(adapter.opened(), adapter.dropped());
}

#[test]
fn batch_catalog_normal_error_and_unwind_paths_close_every_temporary_handle_once() {
    let root_pid = 520;
    let panic_pid = 521;
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![
            Ok(candidate(root_pid, "root.exe", None)),
            Ok(candidate(panic_pid, "panic-fixture.exe", Some(root_pid))),
        ])
        .with_process(
            root_pid,
            planning_script(52_000, "root.exe", r"C:\Fixtures\root.exe"),
        )
        .with_process(
            panic_pid,
            successful_script(
                52_100,
                "panic-fixture.exe",
                r"C:\Fixtures\panic-fixture.exe",
            ),
        )
        .mutate_after(
            FakeCall::Observation(panic_pid),
            1,
            FakeMutation::ResumeUnwind("batch catalog unwind fixture"),
        );

    let unwind = catch_unwind(AssertUnwindSafe(|| {
        let _ = authority(adapter.clone())
            .prepare_batch_planning(&target(root_pid, 52_000), ProcessBatchOperation::Terminate);
    }));

    assert!(unwind.is_err());
    assert_eq!(adapter.catalog_opened(), 1);
    assert_eq!(adapter.catalog_dropped(), 1);
    assert!(!adapter.catalog_active());
    assert_eq!(adapter.opened(), 3);
    assert_eq!(adapter.dropped(), 3);
    let dropped = adapter
        .handle_events()
        .into_iter()
        .filter_map(|event| match event {
            FakeHandleEvent::Dropped { handle_id, .. } => Some(handle_id),
            FakeHandleEvent::Opened { .. } => None,
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(dropped.len(), 3);
}
