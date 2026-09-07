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

mod plan_is_not_deserialize_owned {
    use super::ProcessBatchPlan;

    trait AmbiguousIfImpl<A> {
        fn marker() {}
    }
    impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
    struct Implemented;
    impl<T: ?Sized + serde::de::DeserializeOwned> AmbiguousIfImpl<Implemented> for T {}

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

mod preview_is_not_deserialize_owned {
    use super::ProcessBatchPreview;

    trait AmbiguousIfImpl<A> {
        fn marker() {}
    }
    impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
    struct Implemented;
    impl<T: ?Sized + serde::de::DeserializeOwned> AmbiguousIfImpl<Implemented> for T {}

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

fn caller_visible_type_leaks(
    source: &str,
    sensitive_roots: &[&str],
    impl_owners: &[&str],
) -> Result<(), Vec<String>> {
    let definitions = source_type_definitions(source);
    let mut sensitive = sensitive_roots
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    loop {
        let mut added = false;
        for (name, definition) in &definitions {
            if sensitive.contains(name) {
                continue;
            }
            if identifiers(definition).any(|identifier| sensitive.contains(identifier)) {
                added |= sensitive.insert(name.clone());
            }
        }
        if !added {
            break;
        }
    }

    let visible_source = if impl_owners.is_empty() {
        source.to_owned()
    } else {
        impl_owners
            .iter()
            .map(|owner| {
                braced_item(source, &format!("impl {owner}"))
                    .unwrap_or_else(|| panic!("missing impl {owner}"))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let leaks = caller_visible_signatures(&visible_source)
        .into_iter()
        .filter(|signature| identifiers(signature).any(|identifier| sensitive.contains(identifier)))
        .collect::<Vec<_>>();
    if leaks.is_empty() {
        Ok(())
    } else {
        Err(leaks)
    }
}

fn source_type_definitions(source: &str) -> Vec<(String, String)> {
    let lines = source.lines().collect::<Vec<_>>();
    let mut definitions = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let Some((kind, name)) = type_declaration(lines[index]) else {
            index += 1;
            continue;
        };
        let mut definition = String::new();
        let mut brace_depth = 0_isize;
        let mut saw_brace = false;
        loop {
            let line = lines[index];
            definition.push_str(line);
            definition.push('\n');
            for character in line.chars() {
                match character {
                    '{' => {
                        saw_brace = true;
                        brace_depth += 1;
                    }
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }
            let finished = match kind {
                "type" => line.contains(';'),
                "struct" | "enum" => {
                    (saw_brace && brace_depth == 0) || (!saw_brace && line.contains(';'))
                }
                _ => unreachable!(),
            };
            index += 1;
            if finished || index == lines.len() {
                break;
            }
        }
        definitions.push((name, definition));
    }
    definitions
}

fn type_declaration(line: &str) -> Option<(&'static str, String)> {
    let tokens = line.trim_start().split_whitespace().collect::<Vec<_>>();
    for kind in ["struct", "enum", "type"] {
        let Some(position) = tokens.iter().position(|token| *token == kind) else {
            continue;
        };
        let raw_name = tokens.get(position + 1)?;
        let name = raw_name
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
            .collect::<String>();
        if !name.is_empty() {
            return Some((kind, name));
        }
    }
    None
}

fn identifiers(source: &str) -> impl Iterator<Item = &str> {
    source
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .filter(|token| !token.is_empty())
}

fn braced_item(source: &str, marker: &str) -> Option<String> {
    let start = source.find(marker)?;
    let item = &source[start..];
    let opening = item.find('{')?;
    let mut depth = 0_isize;
    for (offset, character) in item[opening..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(item[..opening + offset + 1].to_owned());
                }
            }
            _ => {}
        }
    }
    None
}

fn caller_visible_signatures(source: &str) -> Vec<String> {
    let lines = source.lines().collect::<Vec<_>>();
    let mut signatures = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if !lines[index].trim_start().starts_with("pub(crate)") {
            index += 1;
            continue;
        }

        let mut signature = String::new();
        loop {
            let line = lines[index];
            signature.push_str(line.trim());
            signature.push(' ');
            index += 1;
            let trimmed = line.trim_end();
            let is_function = signature.contains(" fn ") || signature.starts_with("pub(crate) fn ");
            let finished = if is_function {
                trimmed.contains('{') || trimmed.ends_with(';')
            } else {
                trimmed.contains('{') || trimmed.ends_with(';') || trimmed.ends_with(',')
            };
            if finished || index == lines.len() {
                break;
            }
        }
        signatures.push(signature.trim().to_owned());
    }
    signatures
}

#[test]
fn batch_catalog_api_leak_guard_rejects_aliases_wrappers_and_alternate_accessors() {
    let renamed_target_accessor = r#"
type ConfirmedIdentity = ProcessTarget;
impl ProcessBatchPlan {
    pub(crate) fn confirmed_identity(&self) -> &ConfirmedIdentity {
        unreachable!()
    }
}
"#;
    assert!(caller_visible_type_leaks(
        renamed_target_accessor,
        &["ProcessTarget"],
        &["ProcessBatchPlan"]
    )
    .is_err());

    let renamed_expansion_wrapper = r#"
struct CatalogEnvelope {
    snapshot: ExpansionSnapshot,
}
type InternalCatalog = CatalogEnvelope;
pub(crate) type CallerCatalog = InternalCatalog;
"#;
    assert!(caller_visible_type_leaks(
        renamed_expansion_wrapper,
        &[
            "ExpansionSnapshot",
            "ExpansionEntry",
            "ExpansionOmissionReason",
        ],
        &[],
    )
    .is_err());

    let internal_only = r#"
struct CatalogEnvelope {
    snapshot: ExpansionSnapshot,
}
pub(super) fn prepare_catalog() -> CatalogEnvelope {
    unreachable!()
}
"#;
    assert!(caller_visible_type_leaks(
        internal_only,
        &[
            "ExpansionSnapshot",
            "ExpansionEntry",
            "ExpansionOmissionReason",
        ],
        &[],
    )
    .is_ok());
}

#[test]
fn batch_catalog_contract_is_opaque_redacted_and_side_effect_free() {
    plan_is_not_clone::assert_contract();
    plan_is_not_serialize::assert_contract();
    plan_is_not_deserialize_owned::assert_contract();
    preview_is_not_clone::assert_contract();
    preview_is_not_serialize::assert_contract();
    preview_is_not_deserialize_owned::assert_contract();

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
    let core_source = include_str!("../core.rs");
    let module_source = include_str!("../mod.rs");
    let process_control_source = format!("{core_source}\n{batch_source}\n{module_source}");
    let plan_fields = batch_source
        .split("pub(crate) struct ProcessBatchPlan")
        .nth(1)
        .expect("ProcessBatchPlan must be declared")
        .split("impl ProcessBatchPlan")
        .next()
        .expect("ProcessBatchPlan accessors must follow the fields");
    let preview_fields = batch_source
        .split("pub(crate) struct ProcessBatchPreview")
        .nth(1)
        .expect("ProcessBatchPreview must be declared")
        .split("impl ProcessBatchPreview")
        .next()
        .expect("ProcessBatchPreview accessors must follow the fields");

    assert!(plan_fields.contains("targets: Vec<ProcessTarget>"));
    caller_visible_type_leaks(
        &process_control_source,
        &["ProcessTarget"],
        &["ProcessBatchPlan", "ProcessBatchPreview"],
    )
    .expect("plan and preview signatures must not expose exact targets");
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
    caller_visible_type_leaks(
        &process_control_source,
        &[
            "ExpansionSnapshot",
            "ExpansionEntry",
            "ExpansionOmissionReason",
        ],
        &[],
    )
    .expect("caller-visible process_control signatures must not expose expansion data");

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
