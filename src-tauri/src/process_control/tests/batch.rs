use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    path::PathBuf,
    time::{Duration, Instant},
};

use super::*;
use crate::process_control::batch::{
    build_same_name_plan, build_tree_plan, ProcessBatchCompletion, ProcessBatchError,
    ProcessBatchOperation, ProcessBatchPlan, ProcessBatchPreview, ProcessBatchScope,
    ProcessBatchSelectionCompleteness, ProcessBatchTargetError, ProcessBatchTargetOutcome,
    ProcessBatchTargetResult,
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

fn prepared_same_name(
    root_pid: u32,
    root_creation_time: u64,
    candidates: Vec<ProcessCandidate>,
    processes: Vec<(u32, FakeProcessScript)>,
    comparisons: Vec<Result<bool, ObservationFailure>>,
) -> (FakeProcessAdapter, ProcessBatchPlan) {
    let mut adapter = FakeProcessAdapter::default()
        .with_enumeration(
            std::iter::once(Ok(candidate(root_pid, "root.exe", None)))
                .chain(candidates.into_iter().map(Ok))
                .collect(),
        )
        .with_process_name_equivalences(comparisons)
        .with_process(
            root_pid,
            planning_script(root_creation_time, "root.exe", r"C:\Fixtures\root.exe"),
        );
    for (pid, script) in processes {
        adapter = adapter.with_process(pid, script);
    }
    let prepared = authority(adapter.clone())
        .prepare_batch_planning(
            &target(root_pid, root_creation_time),
            ProcessBatchOperation::Terminate,
        )
        .unwrap();
    let plan = build_same_name_plan(&authority(adapter.clone()), prepared).unwrap();
    (adapter, plan)
}

#[test]
fn batch_same_name_uses_frozen_root_and_adapter_equivalence_ignoring_path() {
    let root_pid = 800;
    let equivalent_pid = 801;
    let unequal_pid = 802;
    let comparisons = vec![
        Ok(true),
        Ok(true), // root snapshot validation
        Ok(true),
        Ok(true), // equivalent snapshot validation
        Ok(true),
        Ok(true),  // unequal snapshot validation
        Ok(true),  // adapter-equivalent Unicode/case name
        Ok(false), // known nonmatch
    ];
    let (adapter, plan) = prepared_same_name(
        root_pid,
        80_000,
        vec![
            candidate(equivalent_pid, "RöÖT.EXE", None),
            candidate(unequal_pid, "other.exe", None),
        ],
        vec![
            (
                equivalent_pid,
                successful_script(80_100, "RöÖT.EXE", r"D:\Elsewhere\RöÖT.EXE"),
            ),
            (
                unequal_pid,
                successful_script(80_200, "other.exe", r"C:\Fixtures\other.exe"),
            ),
        ],
        comparisons,
    );

    assert_eq!(plan.scope(), ProcessBatchScope::SameName);
    assert_eq!(
        preview_rows(&plan),
        vec![
            (
                0,
                root_pid,
                "root.exe".into(),
                Some(PathBuf::from(r"C:\Fixtures\root.exe")),
                None
            ),
            (
                1,
                equivalent_pid,
                "RöÖT.EXE".into(),
                Some(PathBuf::from(r"D:\Elsewhere\RöÖT.EXE")),
                None
            ),
        ]
    );
    assert_eq!(plan.omissions().total(), 0);
    assert_eq!(adapter.terminate_calls(), 0);
}

#[test]
fn batch_same_name_counts_only_equivalent_omissions_and_comparison_failures() {
    let root_pid = 810;
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![
            Ok(candidate(root_pid, "root.exe", None)),
            Ok(candidate(811, "root.exe", None)),
            Ok(candidate(812, "root.exe", None)),
            Ok(candidate(813, "root.exe", None)),
            Ok(candidate(0, "root.exe", None)),
            Ok(candidate(814, "", None)),
            Ok(candidate(815, "other.exe", None)),
            Ok(candidate(816, "other-identity.exe", None)),
            Ok(candidate(817, "root.exe", None)),
            Ok(candidate(818, "root.exe", None)),
        ])
        .with_process_name_equivalences(vec![
            Ok(true),
            Ok(true),                              // root snapshot validation
            Ok(true),                              // exited hint is equivalent
            Ok(true),                              // denied hint is equivalent
            Err(ObservationFailure::AccessDenied), // comparison itself failed
            Ok(true),                              // zero-pid hint is equivalent
            Ok(false),                             // known nonmatch
            Ok(false),                             // valid identity-unavailable nonmatch
            Err(ObservationFailure::ExitedOrChanged),
            Err(ObservationFailure::IdentityUnavailable),
        ])
        .with_process(
            root_pid,
            planning_script(81_000, "root.exe", r"C:\Fixtures\root.exe"),
        )
        .with_process(
            811,
            FakeProcessScript {
                open: vec![Ok(())].into(),
                creation_times: vec![Ok(81_100)].into(),
                liveness: vec![Ok(Liveness::Exited)].into(),
                ..FakeProcessScript::default()
            },
        )
        .with_process(
            812,
            FakeProcessScript {
                open: vec![Err(ObservationFailure::AccessDenied)].into(),
                ..FakeProcessScript::default()
            },
        )
        .with_process(
            813,
            FakeProcessScript {
                open: vec![Err(ObservationFailure::IdentityUnavailable)].into(),
                ..FakeProcessScript::default()
            },
        )
        .with_process(
            815,
            FakeProcessScript {
                open: vec![Err(ObservationFailure::AccessDenied)].into(),
                ..FakeProcessScript::default()
            },
        )
        .with_process(
            816,
            FakeProcessScript {
                open: vec![Err(ObservationFailure::IdentityUnavailable)].into(),
                ..FakeProcessScript::default()
            },
        )
        .with_process(
            817,
            FakeProcessScript {
                open: vec![Err(ObservationFailure::IdentityUnavailable)].into(),
                ..FakeProcessScript::default()
            },
        )
        .with_process(
            818,
            FakeProcessScript {
                open: vec![Err(ObservationFailure::IdentityUnavailable)].into(),
                ..FakeProcessScript::default()
            },
        );
    let authority = authority(adapter.clone());
    let plan = authority
        .plan_batch(
            &target(root_pid, 81_000),
            ProcessBatchScope::SameName,
            ProcessBatchOperation::Terminate,
        )
        .unwrap();

    assert_eq!(plan.previews().len(), 1);
    assert_eq!(plan.omissions().exited_or_changed(), 1);
    assert_eq!(plan.omissions().inspection_denied(), 1);
    assert_eq!(plan.omissions().identity_unavailable(), 5);
    assert_eq!(plan.omissions().ancestry_unverified(), 0);
    assert_eq!(plan.omissions().total(), 7);
}

#[test]
fn batch_same_name_dispatches_once_freezes_order_and_excludes_future_candidates() {
    let root_pid = 820;
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![
            Ok(candidate(root_pid, "root.exe", None)),
            Ok(candidate(822, "root.exe", None)),
            Ok(candidate(821, "root.exe", None)),
        ])
        .with_enumeration(vec![
            Ok(candidate(root_pid, "root.exe", None)),
            Ok(candidate(823, "root.exe", None)),
        ])
        .with_process(
            root_pid,
            planning_script(82_000, "root.exe", r"C:\root.exe"),
        )
        .with_process(821, successful_script(82_200, "root.exe", r"C:\A\root.exe"))
        .with_process(822, successful_script(82_100, "root.exe", r"C:\B\root.exe"))
        .with_process(
            823,
            successful_script(82_300, "root.exe", r"C:\Future\root.exe"),
        );
    let authority = authority(adapter.clone());
    let plan = authority
        .plan_batch(
            &target(root_pid, 82_000),
            ProcessBatchScope::SameName,
            ProcessBatchOperation::ForceTerminate,
        )
        .unwrap();

    assert_eq!(
        plan.previews()
            .iter()
            .map(ProcessBatchPreview::pid)
            .collect::<Vec<_>>(),
        vec![820, 821, 822]
    );
    assert_eq!(plan.operation(), ProcessBatchOperation::ForceTerminate);
    assert_eq!(
        adapter
            .calls()
            .iter()
            .filter(|call| **call == FakeCall::Enumerate)
            .count(),
        1
    );
}

#[test]
fn batch_planning_facade_dispatches_tree_with_one_preparation() {
    let root_pid = 830;
    let child_pid = 831;
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![
            Ok(candidate(root_pid, "root.exe", None)),
            Ok(candidate(child_pid, "child.exe", Some(root_pid))),
        ])
        .with_process(
            root_pid,
            planning_script(83_000, "root.exe", r"C:\Fixtures\root.exe"),
        )
        .with_process(
            child_pid,
            successful_script(83_100, "child.exe", r"C:\Fixtures\child.exe"),
        );
    let plan = authority(adapter.clone())
        .plan_batch(
            &target(root_pid, 83_000),
            ProcessBatchScope::Tree,
            ProcessBatchOperation::Terminate,
        )
        .unwrap();

    assert_eq!(plan.scope(), ProcessBatchScope::Tree);
    assert_eq!(
        plan.previews()
            .iter()
            .map(ProcessBatchPreview::pid)
            .collect::<Vec<_>>(),
        vec![child_pid, root_pid]
    );
    assert_eq!(
        adapter
            .calls()
            .iter()
            .filter(|call| **call == FakeCall::Enumerate)
            .count(),
        1
    );
}

#[test]
fn batch_same_name_accepts_256_targets_and_rejects_257() {
    for (additional, expected_error) in [
        (255_u32, None),
        (256, Some(ProcessBatchError::TargetSetTooLarge)),
    ] {
        let root_pid = 900;
        let mut enumeration = vec![Ok(candidate(root_pid, "root.exe", None))];
        let mut adapter = FakeProcessAdapter::default().with_process(
            root_pid,
            planning_script(90_000, "root.exe", r"C:\root.exe"),
        );
        for offset in 1..=additional {
            let pid = root_pid + offset;
            enumeration.push(Ok(candidate(pid, "root.exe", None)));
            adapter = adapter.with_process(
                pid,
                successful_script(90_000 + offset as u64, "root.exe", r"C:\Same\root.exe"),
            );
        }
        adapter = adapter.with_enumeration(enumeration);
        let authority = authority(adapter.clone());
        let result = authority.plan_batch(
            &target(root_pid, 90_000),
            ProcessBatchScope::SameName,
            ProcessBatchOperation::Terminate,
        );
        assert_eq!(result.as_ref().err().copied(), expected_error);
        if expected_error.is_none() {
            assert_eq!(result.unwrap().previews().len(), 256);
        }
        assert_eq!(adapter.terminate_calls(), 0);
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

#[derive(Clone, Copy)]
enum CatalogVisibility {
    Caller,
    Internal,
}

fn declaration_catalog(source: &str, visibility: CatalogVisibility) -> Vec<String> {
    let lines = source.lines().collect::<Vec<_>>();
    let mut declarations = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim_start();
        let visible = match visibility {
            CatalogVisibility::Caller => {
                line.starts_with("pub(crate) ")
                    || line.starts_with("pub(in crate) ")
                    || line.starts_with("pub ")
            }
            CatalogVisibility::Internal => line.starts_with("pub(super) "),
        };
        if !visible {
            index += 1;
            continue;
        }
        let mut declaration = String::new();
        loop {
            let line = lines[index].trim();
            declaration.push_str(line);
            declaration.push(' ');
            index += 1;
            if line.contains('{') || line.ends_with(';') || index == lines.len() {
                break;
            }
        }
        declarations.push(normalize_declaration(&declaration));
    }
    declarations.sort();
    declarations
}

fn type_alias_catalog(source: &str) -> Vec<String> {
    let lines = source.lines().collect::<Vec<_>>();
    let mut aliases = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim_start();
        let mut tokens = identifier_tokens(line.split('=').next().unwrap_or(line));
        let first = tokens.next();
        if first != Some("type") && !(first == Some("pub") && tokens.any(|token| token == "type")) {
            index += 1;
            continue;
        }
        let mut declaration = String::new();
        loop {
            let line = lines[index].trim();
            declaration.push_str(line);
            declaration.push(' ');
            index += 1;
            if line.ends_with(';') || index == lines.len() {
                break;
            }
        }
        aliases.push(normalize_declaration(&declaration));
    }
    aliases.sort();
    aliases
}

fn protected_owner_import_alias_catalog(source: &str) -> Vec<String> {
    let lines = source.lines().collect::<Vec<_>>();
    let mut aliases = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim_start();
        let item = if let Some(restricted) = line.strip_prefix("pub(") {
            restricted
                .split_once(')')
                .map_or("", |(_, rest)| rest.trim_start())
        } else {
            line.strip_prefix("pub ").unwrap_or(line)
        };
        if !item.starts_with("use ") {
            index += 1;
            continue;
        }
        let mut declaration = String::new();
        loop {
            let line = lines[index].trim();
            declaration.push_str(line);
            declaration.push(' ');
            index += 1;
            if line.ends_with(';') || index == lines.len() {
                break;
            }
        }
        let tokens = identifier_tokens(&declaration).collect::<Vec<_>>();
        if tokens.windows(3).any(|window| {
            matches!(window[0], "ProcessBatchPlan" | "ProcessBatchPreview") && window[1] == "as"
        }) {
            aliases.push(normalize_declaration(&declaration));
        }
    }
    aliases.sort();
    aliases
}

fn relevant_impl_catalog(source: &str) -> Vec<String> {
    let lines = source.lines().collect::<Vec<_>>();
    let mut implementations = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim_start();
        if !line.starts_with("impl ") && !line.starts_with("unsafe impl ") {
            index += 1;
            continue;
        }
        let mut header = String::new();
        loop {
            let line = lines[index].trim();
            header.push_str(line);
            header.push(' ');
            index += 1;
            if line.contains('{') || index == lines.len() {
                break;
            }
        }
        let header = normalize_declaration(&header);
        if [
            "ProcessBatchPlan",
            "ProcessBatchPreview",
            "ProcessBatchReport",
            "ProcessBatchTargetResult",
        ]
        .iter()
        .any(|name| identifier_tokens(&header).any(|token| token == *name))
        {
            implementations.push(header);
        }
    }
    implementations.sort();
    implementations
}

fn normalize_declaration(source: &str) -> String {
    let source = source.split('{').next().unwrap_or(source);
    let mut normalized = source
        .trim()
        .trim_end_matches([';', ','])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for (from, to) in [("( ", "("), (", )", ")"), (" )", ")")] {
        normalized = normalized.replace(from, to);
    }
    normalized
}

fn normalized_item(source: &str, marker: &str) -> String {
    let start = source
        .find(marker)
        .unwrap_or_else(|| panic!("missing {marker}"));
    let item = &source[start..];
    let opening = item.find('{').expect("item must have a body");
    let mut depth = 0_isize;
    for (offset, character) in item[opening..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return item[..opening + offset + 1]
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                }
            }
            _ => {}
        }
    }
    panic!("unterminated {marker}");
}

fn source_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start = source
        .find(start)
        .unwrap_or_else(|| panic!("missing {start}"));
    let source = &source[start..];
    let end = source.find(end).unwrap_or_else(|| panic!("missing {end}"));
    &source[..end]
}

fn expected_catalog(entries: &[&str]) -> Vec<String> {
    let mut entries = entries
        .iter()
        .map(|entry| normalize_declaration(entry))
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

fn returns_exact_target(signature: &str) -> bool {
    normalize_declaration(signature)
        .split_once("->")
        .is_some_and(|(_, output)| identifier_tokens(output).any(|token| token == "ProcessTarget"))
}

fn identifier_tokens(source: &str) -> impl Iterator<Item = &str> {
    source
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|token| !token.is_empty())
}

#[test]
fn batch_catalog_api_leak_guard_rejects_aliases_wrappers_and_alternate_accessors() {
    let approved_impl = "impl ProcessBatchPlan {}";
    let second_inherent_impl = r#"
impl ProcessBatchPlan {
}
impl ProcessBatchPlan {
    pub(crate) fn exact_identity(&self) -> &ProcessTarget {
        unreachable!()
    }
}
"#;
    assert_eq!(
        relevant_impl_catalog(approved_impl),
        expected_catalog(&["impl ProcessBatchPlan"])
    );
    assert_ne!(
        relevant_impl_catalog(second_inherent_impl),
        relevant_impl_catalog(approved_impl)
    );

    let free_function_seam = r#"
pub(crate) fn inspect_plan(
    plan: &ProcessBatchPlan,
) -> &ProcessTarget {
    unreachable!()
}
"#;
    assert!(!declaration_catalog(free_function_seam, CatalogVisibility::Caller).is_empty());
    assert!(returns_exact_target(free_function_seam));

    let crate_visible_free_function_seam = r#"
pub(in crate) fn exact_identity(
    plan: &ProcessBatchPlan,
) -> &ProcessTarget {
    unreachable!()
}
"#;
    assert_eq!(
        declaration_catalog(crate_visible_free_function_seam, CatalogVisibility::Caller),
        expected_catalog(&[
            "pub(in crate) fn exact_identity(plan: &ProcessBatchPlan) -> &ProcessTarget"
        ])
    );
    assert!(returns_exact_target(crate_visible_free_function_seam));

    let module_visible_free_function_seam = r#"
pub(in crate::process_control) fn exact_identity(
    plan: &ProcessBatchPlan,
) -> &ProcessTarget {
    unreachable!()
}
"#;
    assert!(
        declaration_catalog(module_visible_free_function_seam, CatalogVisibility::Caller)
            .is_empty()
    );

    let permitted_planning_input = r#"
pub(crate) fn plan_batch(root: &ProcessTarget) -> ProcessBatchPlan {
    unreachable!()
}
"#;
    assert!(!returns_exact_target(permitted_planning_input));
    assert_eq!(
        declaration_catalog(permitted_planning_input, CatalogVisibility::Caller),
        expected_catalog(&["pub(crate) fn plan_batch(root: &ProcessTarget) -> ProcessBatchPlan"])
    );

    let trait_seam = r#"
pub(crate) trait PlanInspection {
    fn exact_identity(&self) -> &ProcessTarget;
}
impl PlanInspection for ProcessBatchPlan {
    fn exact_identity(&self) -> &ProcessTarget {
        unreachable!()
    }
}
"#;
    assert!(!declaration_catalog(trait_seam, CatalogVisibility::Caller).is_empty());
    assert_ne!(
        relevant_impl_catalog(trait_seam),
        relevant_impl_catalog(approved_impl)
    );

    let private_self_alias_trait_impl = r#"
type HiddenPlan = ProcessBatchPlan;
impl PublicTrait for HiddenPlan {}
"#;
    assert_ne!(
        type_alias_catalog(private_self_alias_trait_impl),
        Vec::<String>::new()
    );

    let private_owner_import_alias = r#"
use self::ProcessBatchPlan as HiddenPlan;
impl Deref for HiddenPlan {
    type Target = ProcessTarget;
}
"#;
    assert_eq!(
        protected_owner_import_alias_catalog(private_owner_import_alias),
        expected_catalog(&["use self::ProcessBatchPlan as HiddenPlan"])
    );

    let private_preview_import_alias = r#"
use self::ProcessBatchPreview as HiddenPreview;
"#;
    assert_eq!(
        protected_owner_import_alias_catalog(private_preview_import_alias),
        expected_catalog(&["use self::ProcessBatchPreview as HiddenPreview"])
    );

    let visibility_prefixed_owner_import_aliases = r#"
pub use self::ProcessBatchPlan as PublicPlan;
pub(crate) use self::ProcessBatchPreview as CratePreview;
pub(super) use self::ProcessBatchPlan as ParentPlan;
pub(super) use self::ProcessBatchPreview as ParentPreview;
pub(in crate::process_control) use self::ProcessBatchPlan as ScopedPlan;
"#;
    assert_eq!(
        protected_owner_import_alias_catalog(visibility_prefixed_owner_import_aliases),
        expected_catalog(&[
            "pub use self::ProcessBatchPlan as PublicPlan",
            "pub(crate) use self::ProcessBatchPreview as CratePreview",
            "pub(super) use self::ProcessBatchPlan as ParentPlan",
            "pub(super) use self::ProcessBatchPreview as ParentPreview",
            "pub(in crate::process_control) use self::ProcessBatchPlan as ScopedPlan",
        ])
    );

    let expansion_reexport = r#"
pub(crate) use crate::process_control::core::ExpansionSnapshot as HiddenCatalog;
"#;
    assert_eq!(
        declaration_catalog(expansion_reexport, CatalogVisibility::Caller),
        expected_catalog(&[
            "pub(crate) use crate::process_control::core::ExpansionSnapshot as HiddenCatalog"
        ])
    );

    let internal_only = r#"
struct CatalogEnvelope {
    snapshot: ExpansionSnapshot,
}
pub(super) fn prepare_catalog() -> CatalogEnvelope {
    unreachable!()
}
"#;
    assert!(declaration_catalog(internal_only, CatalogVisibility::Caller).is_empty());
    assert_eq!(
        declaration_catalog(internal_only, CatalogVisibility::Internal),
        expected_catalog(&["pub(super) fn prepare_catalog() -> CatalogEnvelope"])
    );

    let approved_preview = r#"
pub(crate) struct ProcessBatchPreview {
    ordinal: usize,
    pid: u32,
    name: String,
    executable_path: Option<PathBuf>,
    depth: Option<usize>,
}
"#;
    let alias_backed_preview_field = r#"
pub(crate) struct ProcessBatchPreview {
    ordinal: usize,
    pid: u32,
    name: String,
    executable_path: Option<PathBuf>,
    depth: Option<usize>,
    hidden_identity: HiddenIdentity,
}
"#;
    assert_ne!(
        normalized_item(
            alias_backed_preview_field,
            "pub(crate) struct ProcessBatchPreview"
        ),
        normalized_item(approved_preview, "pub(crate) struct ProcessBatchPreview")
    );
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
    assert_eq!(type_alias_catalog(batch_source), Vec::<String>::new());
    assert_eq!(
        protected_owner_import_alias_catalog(batch_source),
        Vec::<String>::new()
    );
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
    assert_eq!(
        declaration_catalog(batch_source, CatalogVisibility::Caller),
        expected_catalog(&[
            "pub(crate) enum ProcessBatchCompletion",
            "pub(crate) enum ProcessBatchError",
            "pub(crate) enum ProcessBatchOperation",
            "pub(crate) enum ProcessBatchScope",
            "pub(crate) enum ProcessBatchSelectionCompleteness",
            "pub(crate) enum ProcessBatchTargetError",
            "pub(crate) enum ProcessBatchTargetOutcome",
            "pub(crate) struct ProcessBatchOmissions",
            "pub(crate) struct ProcessBatchPlan",
            "pub(crate) struct ProcessBatchPreview",
            "pub(crate) struct ProcessBatchReport",
            "pub(crate) struct ProcessBatchTargetResult",
            "pub(crate) fn already_exited(&self) -> u64",
            "pub(crate) fn ancestry_unverified(&self) -> u64",
            "pub(crate) fn completion(&self) -> ProcessBatchCompletion",
            "pub(crate) fn depth(&self) -> Option<usize>",
            "pub(crate) fn executable_path(&self) -> Option<&Path>",
            "pub(crate) fn exited_or_changed(&self) -> u64",
            "pub(crate) fn failed(&self) -> u64",
            "pub(crate) fn identity_unavailable(&self) -> u64",
            "pub(crate) fn inspection_denied(&self) -> u64",
            "pub(crate) fn name(&self) -> &str",
            "pub(crate) fn not_attempted(&self) -> u64",
            "pub(crate) fn observed_exited(&self) -> u64",
            "pub(crate) fn omissions(&self) -> &ProcessBatchOmissions",
            "pub(crate) fn omissions(&self) -> &ProcessBatchOmissions",
            "pub(crate) fn operation(&self) -> ProcessBatchOperation",
            "pub(crate) fn operation(&self) -> ProcessBatchOperation",
            "pub(crate) fn ordinal(&self) -> usize",
            "pub(crate) fn outcome(&self) -> ProcessBatchTargetOutcome",
            "pub(crate) fn pid(&self) -> u32",
            "pub(crate) fn preview(&self) -> &ProcessBatchPreview",
            "pub(crate) fn previews(&self) -> &[ProcessBatchPreview]",
            "pub(crate) fn results(&self) -> &[ProcessBatchTargetResult]",
            "pub(crate) fn scope(&self) -> ProcessBatchScope",
            "pub(crate) fn scope(&self) -> ProcessBatchScope",
            "pub(crate) fn selection_completeness(&self) -> ProcessBatchSelectionCompleteness",
            "pub(crate) fn total(&self) -> u64",
        ])
    );
    assert_eq!(
        relevant_impl_catalog(batch_source),
        expected_catalog(&[
            "impl ProcessBatchPlan",
            "impl ProcessBatchPreview",
            "impl ProcessBatchReport",
            "impl ProcessBatchTargetResult",
        ])
    );
    assert!(declaration_catalog(batch_source, CatalogVisibility::Caller)
        .iter()
        .all(|signature| !returns_exact_target(signature)));
    assert_eq!(
        declaration_catalog(module_source, CatalogVisibility::Caller),
        expected_catalog(&[
            "pub(crate) type HostInstanceId = core::HostInstanceId",
            "pub(crate) type ProcessAuthorityInitError = core::ProcessAuthorityInitError",
            "pub(crate) type ProcessError = core::ProcessError",
            "pub(crate) type ProcessOmissions = core::ProcessOmissions",
            "pub(crate) type ProcessRecord = core::ProcessRecord",
            "pub(crate) type ProcessIntent = core::ProcessIntent",
            "pub(crate) type ProcessOperationError = operation::ProcessOperationError",
            "pub(crate) type ProcessSnapshot = core::ProcessSnapshot",
            "pub(crate) type ProcessTarget = core::ProcessTarget",
            "pub(crate) type ProcessBatchError = batch::ProcessBatchError",
            "pub(crate) type ProcessBatchOmissions = batch::ProcessBatchOmissions",
            "pub(crate) type ProcessBatchOperation = batch::ProcessBatchOperation",
            "pub(crate) type ProcessBatchPlan = batch::ProcessBatchPlan",
            "pub(crate) type ProcessBatchPreview = batch::ProcessBatchPreview",
            "pub(crate) type ProcessBatchScope = batch::ProcessBatchScope",
            "pub(crate) type ProcessBatchSelectionCompleteness = batch::ProcessBatchSelectionCompleteness",
            "pub(crate) type ProcessBatchTargetError = batch::ProcessBatchTargetError",
            "pub(crate) type ProcessBatchTargetOutcome = batch::ProcessBatchTargetOutcome",
            "pub(crate) type ProcessBatchTargetResult = batch::ProcessBatchTargetResult",
            "pub(crate) type ProcessBatchReport = batch::ProcessBatchReport",
            "pub(crate) type ProcessBatchCompletion = batch::ProcessBatchCompletion",
            "pub(crate) struct ProcessAuthority",
            "pub(crate) struct ProcessAuthority",
            "pub(crate) struct ProcessAdmission",
            "pub(crate) enum ProcessAdmission",
            "pub(crate) fn new() -> Result<Self, ProcessAuthorityInitError>",
            "pub(crate) fn new() -> Result<Self, ProcessAuthorityInitError>",
            "pub(crate) fn snapshot(&self) -> Result<ProcessSnapshot, ProcessError>",
            "pub(crate) fn snapshot(&self) -> Result<ProcessSnapshot, ProcessError>",
            "pub(crate) fn plan_batch(&self, root: &ProcessTarget, scope: ProcessBatchScope, operation: ProcessBatchOperation) -> Result<ProcessBatchPlan, ProcessBatchError>",
            "pub(crate) fn plan_batch(&self, _root: &ProcessTarget, _scope: ProcessBatchScope, _operation: ProcessBatchOperation) -> Result<ProcessBatchPlan, ProcessBatchError>",
            "pub(crate) fn execute_batch(&self, plan: ProcessBatchPlan) -> Result<ProcessBatchReport, ProcessBatchError>",
            "pub(crate) fn execute_batch(&self, _plan: ProcessBatchPlan) -> Result<ProcessBatchReport, ProcessBatchError>",
            "pub(crate) fn admit(&self, target: &ProcessTarget, intent: ProcessIntent) -> Result<ProcessAdmission, ProcessError>",
            "pub(crate) fn admit(&self, _target: &ProcessTarget, _intent: ProcessIntent) -> Result<ProcessAdmission, ProcessError>",
            "pub(crate) fn terminate(&self, admission: ProcessAdmission) -> Result<(), ProcessOperationError>",
            "pub(crate) fn terminate(&self, _admission: ProcessAdmission) -> Result<(), ProcessOperationError>",
            "pub(crate) fn force_terminate(&self, admission: ProcessAdmission) -> Result<(), ProcessOperationError>",
            "pub(crate) fn force_terminate(&self, _admission: ProcessAdmission) -> Result<(), ProcessOperationError>",
        ])
    );
    for (marker, expected) in [
        (
            "pub(crate) struct ProcessBatchOmissions",
            "pub(crate) struct ProcessBatchOmissions { exited_or_changed: u64, inspection_denied: u64, identity_unavailable: u64, ancestry_unverified: u64, }",
        ),
        (
            "pub(crate) struct ProcessBatchPlan",
            "pub(crate) struct ProcessBatchPlan { authority_id: HostInstanceId, scope: ProcessBatchScope, operation: ProcessBatchOperation, targets: Vec<ProcessTarget>, previews: Vec<ProcessBatchPreview>, omissions: ProcessBatchOmissions, }",
        ),
        (
            "pub(crate) struct ProcessBatchPreview",
            "pub(crate) struct ProcessBatchPreview { ordinal: usize, pid: u32, name: String, executable_path: Option<std::path::PathBuf>, depth: Option<usize>, }",
        ),
        (
            "pub(crate) enum ProcessBatchScope",
            "pub(crate) enum ProcessBatchScope { Tree, SameName, }",
        ),
        (
            "pub(crate) enum ProcessBatchOperation",
            "pub(crate) enum ProcessBatchOperation { Terminate, ForceTerminate, }",
        ),
        (
            "pub(crate) enum ProcessBatchSelectionCompleteness",
            "pub(crate) enum ProcessBatchSelectionCompleteness { CompleteSelection, IncompleteSelection, }",
        ),
        (
            "pub(crate) enum ProcessBatchError",
            "pub(crate) enum ProcessBatchError { InvalidTarget, ExpiredTarget, TargetExited, TargetChanged, InspectionDenied, ProtectedTarget, CatalogUnavailable, TargetSetTooLarge, ExpiredPlan, PlatformUnsupported, }",
        ),
    ] {
        assert_eq!(
            normalized_item(batch_source, marker),
            expected.split_whitespace().collect::<Vec<_>>().join(" ")
        );
    }
    let expansion_source = source_between(
        core_source,
        "pub(super) struct ExpansionSnapshot",
        "pub(crate) enum ProcessIntent",
    );
    assert_eq!(
        declaration_catalog(expansion_source, CatalogVisibility::Internal),
        expected_catalog(&[
            "pub(super) struct ExpansionSnapshot",
            "pub(super) fn entries(&self) -> &[ExpansionEntry]",
            "pub(super) struct ExpansionEntry",
            "pub(super) fn pid_hint(&self) -> u32",
            "pub(super) fn name_hint(&self) -> &str",
            "pub(super) fn observed_parent_pid_hint(&self) -> Option<u32>",
            "pub(super) fn record(&self) -> Option<&ProcessRecord>",
            "pub(super) fn omission_reason(&self) -> Option<ExpansionOmissionReason>",
            "pub(super) enum ExpansionOmissionReason",
        ])
    );
    for (marker, expected) in [
        (
            "pub(super) struct ExpansionSnapshot",
            "pub(super) struct ExpansionSnapshot { entries: Vec<ExpansionEntry>, }",
        ),
        (
            "pub(super) struct ExpansionEntry",
            "pub(super) struct ExpansionEntry { pid_hint: u32, name_hint: String, observed_parent_pid_hint: Option<u32>, record: Option<ProcessRecord>, omission_reason: Option<ExpansionOmissionReason>, }",
        ),
        (
            "pub(super) enum ExpansionOmissionReason",
            "pub(super) enum ExpansionOmissionReason { ExitedOrChanged, InspectionDenied, IdentityUnavailable, }",
        ),
    ] {
        assert_eq!(
            normalized_item(core_source, marker),
            expected.split_whitespace().collect::<Vec<_>>().join(" ")
        );
    }
    let prepared_source = source_between(
        batch_source,
        "pub(super) struct PreparedBatchPlanning",
        "impl From<ProcessError>",
    );
    assert_eq!(
        declaration_catalog(prepared_source, CatalogVisibility::Internal),
        expected_catalog(&[
            "pub(super) struct PreparedBatchPlanning",
            "pub(super) fn authority_id(&self) -> HostInstanceId",
            "pub(super) fn root(&self) -> &ProcessRecord",
            "pub(super) fn snapshot(&self) -> &ExpansionSnapshot",
            "pub(super) fn operation(&self) -> ProcessBatchOperation",
            "pub(super) fn plan_batch(&self, root: &ProcessTarget, scope: ProcessBatchScope, operation: ProcessBatchOperation) -> Result<ProcessBatchPlan, ProcessBatchError>",
            "pub(super) fn prepare_batch_planning(&self, root: &ProcessTarget, operation: ProcessBatchOperation) -> Result<PreparedBatchPlanning, ProcessBatchError>",
            "pub(super) fn build_same_name_plan<A: ProcessPlatformAdapter>(authority: &CoreAuthority<A>, prepared: PreparedBatchPlanning) -> Result<ProcessBatchPlan, ProcessBatchError>",
            "pub(super) fn build_tree_plan(prepared: PreparedBatchPlanning) -> Result<ProcessBatchPlan, ProcessBatchError>",
            "pub(super) fn execute_batch_with_clock<C: MonotonicClock>(&self, plan: ProcessBatchPlan, clock: &C) -> Result<ProcessBatchReport, ProcessBatchError>",
            "pub(super) fn map_admission_outcome(error: ProcessError) -> ProcessBatchTargetOutcome",
            "pub(super) fn map_operation_outcome(error: ProcessOperationError) -> ProcessBatchTargetOutcome",
        ])
    );

    let planning_facade = declaration_catalog(module_source, CatalogVisibility::Caller)
        .into_iter()
        .filter(|signature| signature.contains("fn plan_batch"))
        .collect::<Vec<_>>();
    assert_eq!(planning_facade.len(), 2);
    assert!(planning_facade.iter().all(|signature| {
        signature.contains("root: &ProcessTarget")
            && signature.contains("scope: ProcessBatchScope")
            && signature.contains("operation: ProcessBatchOperation")
            && signature.contains("Result<ProcessBatchPlan, ProcessBatchError>")
            && !signature.contains("name:")
            && !signature.contains("parent")
            && !signature.contains("Vec<ProcessTarget>")
    }));
    let non_windows_facade = source_between(
        module_source,
        "#[cfg(not(windows))]\npub(crate) struct ProcessAuthority",
        "#[cfg(not(windows))]\npub(crate) enum ProcessAdmission",
    );
    assert!(non_windows_facade.contains("Err(ProcessBatchError::PlatformUnsupported)"));
    assert!(!batch_source.contains("execute_batch("));
    assert!(!batch_source.contains("reenumerate_name("));
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

fn prepared_tree(
    root_pid: u32,
    root_creation_time: u64,
    candidates: Vec<ProcessCandidate>,
    processes: Vec<(u32, FakeProcessScript)>,
) -> (FakeProcessAdapter, ProcessBatchPlan) {
    let mut adapter = FakeProcessAdapter::default().with_enumeration(
        std::iter::once(Ok(candidate(root_pid, "root.exe", None)))
            .chain(candidates.into_iter().map(Ok))
            .collect(),
    );
    adapter = adapter.with_process(
        root_pid,
        planning_script(root_creation_time, "root.exe", r"C:\Fixtures\root.exe"),
    );
    for (pid, script) in processes {
        adapter = adapter.with_process(pid, script);
    }
    let prepared = authority(adapter.clone())
        .prepare_batch_planning(
            &target(root_pid, root_creation_time),
            ProcessBatchOperation::Terminate,
        )
        .unwrap();
    let plan = build_tree_plan(prepared).unwrap();
    (adapter, plan)
}

fn preview_rows(
    plan: &ProcessBatchPlan,
) -> Vec<(usize, u32, String, Option<PathBuf>, Option<usize>)> {
    plan.previews()
        .iter()
        .map(|preview| {
            (
                preview.ordinal(),
                preview.pid(),
                preview.name().to_owned(),
                preview.executable_path().map(PathBuf::from),
                preview.depth(),
            )
        })
        .collect()
}

#[test]
fn batch_tree_root_only_freezes_exact_preview_without_operation_calls() {
    let (adapter, plan) = prepared_tree(600, 60_000, vec![], vec![]);

    assert_eq!(plan.scope(), ProcessBatchScope::Tree);
    assert_eq!(plan.operation(), ProcessBatchOperation::Terminate);
    assert_eq!(
        preview_rows(&plan),
        vec![(
            0,
            600,
            "root.exe".into(),
            Some(PathBuf::from(r"C:\Fixtures\root.exe")),
            Some(0),
        )]
    );
    assert_eq!(
        plan.selection_completeness(),
        ProcessBatchSelectionCompleteness::CompleteSelection
    );
    assert_eq!(adapter.requested_operation_access(), vec![]);
    assert_eq!(adapter.post_close_calls(), 0);
    assert_eq!(adapter.terminate_calls(), 0);
}

#[test]
fn batch_tree_orders_deepest_first_then_pid_and_freezes_snapshot_values() {
    let candidates = vec![
        candidate(603, "sibling.exe", Some(600)),
        candidate(602, "child.exe", Some(600)),
        candidate(604, "grandchild.exe", Some(602)),
    ];
    let processes = vec![
        (
            603,
            successful_script(60_300, "sibling.exe", r"C:\Old\sibling.exe"),
        ),
        (
            602,
            successful_script(60_200, "child.exe", r"C:\Old\child.exe"),
        ),
        (
            604,
            successful_script(60_400, "grandchild.exe", r"C:\Old\grandchild.exe"),
        ),
    ];
    let (adapter, plan) = prepared_tree(600, 60_000, candidates, processes);

    adapter.replace_process(
        602,
        successful_script(99_999, "changed.exe", r"C:\New\changed.exe"),
    );
    assert_eq!(
        preview_rows(&plan),
        vec![
            (
                0,
                604,
                "grandchild.exe".into(),
                Some(PathBuf::from(r"C:\Old\grandchild.exe")),
                Some(2)
            ),
            (
                1,
                602,
                "child.exe".into(),
                Some(PathBuf::from(r"C:\Old\child.exe")),
                Some(1)
            ),
            (
                2,
                603,
                "sibling.exe".into(),
                Some(PathBuf::from(r"C:\Old\sibling.exe")),
                Some(1)
            ),
            (
                3,
                600,
                "root.exe".into(),
                Some(PathBuf::from(r"C:\Fixtures\root.exe")),
                Some(0)
            ),
        ]
    );
}

#[test]
fn batch_tree_attributes_direct_failure_and_blocks_descendants_once() {
    let denied = FakeProcessScript {
        open: vec![Err(ObservationFailure::AccessDenied)].into(),
        ..FakeProcessScript::default()
    };
    let (adapter, plan) = prepared_tree(
        610,
        61_000,
        vec![
            candidate(611, "denied.exe", Some(610)),
            candidate(612, "behind.exe", Some(611)),
        ],
        vec![
            (611, denied),
            (
                612,
                successful_script(61_200, "behind.exe", r"C:\Fixtures\behind.exe"),
            ),
        ],
    );

    assert_eq!(plan.previews().len(), 1);
    assert_eq!(plan.omissions().inspection_denied(), 1);
    assert_eq!(plan.omissions().ancestry_unverified(), 1);
    assert_eq!(plan.omissions().total(), 2);
    assert_eq!(
        plan.selection_completeness(),
        ProcessBatchSelectionCompleteness::IncompleteSelection
    );
    assert_eq!(adapter.requested_operation_access(), vec![]);
}

#[test]
fn batch_tree_preserves_each_direct_stage_one_omission_category() {
    let (adapter, plan) = prepared_tree(
        615,
        61_500,
        vec![
            candidate(616, "exited.exe", Some(615)),
            candidate(617, "denied.exe", Some(615)),
            candidate(0, "zero.exe", Some(615)),
        ],
        vec![
            (
                616,
                FakeProcessScript {
                    open: vec![Ok(())].into(),
                    creation_times: vec![Ok(61_600)].into(),
                    liveness: vec![Ok(Liveness::Exited)].into(),
                    ..FakeProcessScript::default()
                },
            ),
            (
                617,
                FakeProcessScript {
                    open: vec![Err(ObservationFailure::AccessDenied)].into(),
                    ..FakeProcessScript::default()
                },
            ),
        ],
    );

    assert_eq!(plan.omissions().exited_or_changed(), 1);
    assert_eq!(plan.omissions().inspection_denied(), 1);
    assert_eq!(plan.omissions().identity_unavailable(), 1);
    assert_eq!(plan.omissions().ancestry_unverified(), 0);
    assert_eq!(plan.omissions().total(), 3);
    assert_eq!(adapter.requested_operation_access(), vec![]);
}

#[test]
fn batch_tree_rejects_time_inversion_missing_parent_self_loop_and_reachable_cycle() {
    let (adapter, plan) = prepared_tree(
        620,
        62_000,
        vec![
            candidate(621, "older.exe", Some(620)),
            candidate(622, "behind-older.exe", Some(621)),
            candidate(623, "missing.exe", Some(9_123)),
            candidate(624, "self.exe", Some(624)),
            candidate(625, "cycle-a.exe", Some(626)),
            candidate(626, "cycle-b.exe", Some(625)),
            candidate(627, "other-root.exe", Some(0)),
            candidate(630, "unreliable-outside.exe", None),
            candidate(631, "behind-unreliable-outside.exe", Some(630)),
        ],
        vec![
            (
                621,
                successful_script(61_999, "older.exe", r"C:\Fixtures\older.exe"),
            ),
            (
                622,
                successful_script(62_200, "behind-older.exe", r"C:\Fixtures\behind-older.exe"),
            ),
            (
                623,
                successful_script(62_300, "missing.exe", r"C:\Fixtures\missing.exe"),
            ),
            (
                624,
                successful_script(62_400, "self.exe", r"C:\Fixtures\self.exe"),
            ),
            (
                625,
                successful_script(62_500, "cycle-a.exe", r"C:\Fixtures\cycle-a.exe"),
            ),
            (
                626,
                successful_script(62_600, "cycle-b.exe", r"C:\Fixtures\cycle-b.exe"),
            ),
            (
                627,
                successful_script(62_700, "other-root.exe", r"C:\Fixtures\other-root.exe"),
            ),
            (
                630,
                FakeProcessScript {
                    open: vec![Err(ObservationFailure::AccessDenied)].into(),
                    ..FakeProcessScript::default()
                },
            ),
            (
                631,
                successful_script(
                    62_710,
                    "behind-unreliable-outside.exe",
                    r"C:\Fixtures\behind-unreliable-outside.exe",
                ),
            ),
        ],
    );

    assert_eq!(
        plan.previews()
            .iter()
            .map(ProcessBatchPreview::pid)
            .collect::<Vec<_>>(),
        vec![620]
    );
    assert_eq!(plan.omissions().ancestry_unverified(), 8);
    assert_eq!(plan.omissions().total(), 8);
    assert_eq!(adapter.requested_operation_access(), vec![]);
}

#[test]
fn batch_tree_root_incoming_edge_blocks_reachable_cycle_without_omitting_root() {
    let root_pid = 628;
    let child_pid = 629;
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![
            Ok(candidate(root_pid, "root.exe", Some(child_pid))),
            Ok(candidate(child_pid, "child.exe", Some(root_pid))),
        ])
        .with_process(
            root_pid,
            planning_script(62_800, "root.exe", r"C:\Fixtures\root.exe"),
        )
        .with_process(
            child_pid,
            successful_script(62_900, "child.exe", r"C:\Fixtures\child.exe"),
        );
    let prepared = authority(adapter.clone())
        .prepare_batch_planning(&target(root_pid, 62_800), ProcessBatchOperation::Terminate)
        .unwrap();
    let plan = build_tree_plan(prepared).unwrap();

    assert_eq!(
        plan.previews()
            .iter()
            .map(ProcessBatchPreview::pid)
            .collect::<Vec<_>>(),
        vec![root_pid]
    );
    assert_eq!(plan.omissions().ancestry_unverified(), 1);
    assert_eq!(plan.omissions().total(), 1);
    assert_eq!(adapter.requested_operation_access(), vec![]);
}

#[test]
fn batch_tree_root_parent_and_its_other_children_are_provably_outside_scope() {
    let root_pid = 635;
    let parent_pid = 636;
    let sibling_pid = 637;
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![
            Ok(candidate(root_pid, "root.exe", Some(parent_pid))),
            Ok(candidate(parent_pid, "parent.exe", None)),
            Ok(candidate(sibling_pid, "sibling.exe", Some(parent_pid))),
        ])
        .with_process(
            root_pid,
            planning_script(63_500, "root.exe", r"C:\Fixtures\root.exe"),
        )
        .with_process(
            parent_pid,
            successful_script(63_400, "parent.exe", r"C:\Fixtures\parent.exe"),
        )
        .with_process(
            sibling_pid,
            successful_script(63_600, "sibling.exe", r"C:\Fixtures\sibling.exe"),
        );
    let prepared = authority(adapter.clone())
        .prepare_batch_planning(&target(root_pid, 63_500), ProcessBatchOperation::Terminate)
        .unwrap();
    let plan = build_tree_plan(prepared).unwrap();

    assert_eq!(
        plan.previews()
            .iter()
            .map(ProcessBatchPreview::pid)
            .collect::<Vec<_>>(),
        vec![root_pid]
    );
    assert_eq!(plan.omissions().total(), 0);
    assert_eq!(adapter.requested_operation_access(), vec![]);
}

#[test]
fn batch_tree_uses_only_prepared_relationships_and_never_reenumerates() {
    let root_pid = 633;
    let child_pid = 634;
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![
            Ok(candidate(root_pid, "root.exe", None)),
            Ok(candidate(child_pid, "child.exe", Some(root_pid))),
        ])
        .with_enumeration(vec![
            Ok(candidate(root_pid, "renamed-root.exe", None)),
            Ok(candidate(child_pid, "renamed-child.exe", None)),
        ])
        .with_process(
            root_pid,
            planning_script(63_300, "root.exe", r"C:\Fixtures\root.exe"),
        )
        .with_process(
            child_pid,
            successful_script(63_400, "child.exe", r"C:\Fixtures\child.exe"),
        );
    let prepared = authority(adapter.clone())
        .prepare_batch_planning(&target(root_pid, 63_300), ProcessBatchOperation::Terminate)
        .unwrap();
    let calls_after_preparation = adapter.calls();
    let plan = build_tree_plan(prepared).unwrap();

    assert_eq!(adapter.calls(), calls_after_preparation);
    assert_eq!(
        adapter
            .calls()
            .iter()
            .filter(|call| **call == FakeCall::Enumerate)
            .count(),
        1
    );
    assert_eq!(
        preview_rows(&plan),
        vec![
            (
                0,
                child_pid,
                "child.exe".into(),
                Some(PathBuf::from(r"C:\Fixtures\child.exe")),
                Some(1),
            ),
            (
                1,
                root_pid,
                "root.exe".into(),
                Some(PathBuf::from(r"C:\Fixtures\root.exe")),
                Some(0),
            ),
        ]
    );
}

#[test]
fn batch_tree_duplicate_pid_branch_is_ancestry_unverified_per_candidate() {
    let duplicate_pid = 631;
    let (adapter, plan) = prepared_tree(
        630,
        63_000,
        vec![
            candidate(duplicate_pid, "first.exe", Some(630)),
            candidate(duplicate_pid, "second.exe", Some(630)),
            candidate(632, "behind.exe", Some(duplicate_pid)),
        ],
        vec![
            (
                duplicate_pid,
                successful_script(63_100, "first.exe", r"C:\Fixtures\first.exe"),
            ),
            (
                632,
                successful_script(63_200, "behind.exe", r"C:\Fixtures\behind.exe"),
            ),
        ],
    );

    assert_eq!(plan.previews().len(), 1);
    assert_eq!(plan.omissions().exited_or_changed(), 0);
    assert_eq!(plan.omissions().ancestry_unverified(), 3);
    assert_eq!(adapter.requested_operation_access(), vec![]);
}

#[test]
fn batch_tree_accepts_256_targets_and_rejects_257_without_operation_calls() {
    for (descendants, expected_error) in [
        (255_u32, None),
        (256, Some(ProcessBatchError::TargetSetTooLarge)),
    ] {
        let root_pid = 700;
        let mut candidates = Vec::new();
        let mut processes = Vec::new();
        for offset in 1..=descendants {
            let pid = root_pid + offset;
            candidates.push(candidate(pid, "child.exe", Some(root_pid)));
            processes.push((
                pid,
                successful_script(
                    70_000 + offset as u64,
                    "child.exe",
                    r"C:\Fixtures\child.exe",
                ),
            ));
        }
        let mut adapter = FakeProcessAdapter::default().with_enumeration(
            std::iter::once(Ok(candidate(root_pid, "root.exe", None)))
                .chain(candidates.into_iter().map(Ok))
                .collect(),
        );
        adapter = adapter.with_process(
            root_pid,
            planning_script(70_000, "root.exe", r"C:\Fixtures\root.exe"),
        );
        for (pid, script) in processes {
            adapter = adapter.with_process(pid, script);
        }
        let prepared = authority(adapter.clone())
            .prepare_batch_planning(&target(root_pid, 70_000), ProcessBatchOperation::Terminate)
            .unwrap();
        let result = build_tree_plan(prepared);
        assert_eq!(result.as_ref().err().copied(), expected_error);
        if expected_error.is_none() {
            assert_eq!(result.unwrap().previews().len(), 256);
        }
        assert_eq!(adapter.requested_operation_access(), vec![]);
        assert_eq!(adapter.terminate_calls(), 0);
    }
}

fn batch_execution_force_script(
    creation_time: u64,
    admission_liveness: Liveness,
    operation_open: Result<(), OperationFailure>,
    termination: Result<(), OperationFailure>,
    wait: Result<ExitWait, OperationFailure>,
) -> FakeProcessScript {
    FakeProcessScript {
        open: vec![Ok(()), Ok(()), Ok(())].into(),
        operation_open: vec![operation_open].into(),
        creation_times: vec![
            Ok(creation_time),
            Ok(creation_time),
            Ok(creation_time),
            Ok(creation_time),
        ]
        .into(),
        liveness: vec![
            Ok(Liveness::Alive),
            Ok(Liveness::Alive),
            Ok(admission_liveness),
            Ok(Liveness::Alive),
        ]
        .into(),
        observations: vec![Ok(path_observation("root.exe", r"C:\Fixtures\root.exe"))].into(),
        protections: vec![Ok(ProcessProtection::new(false, false)); 3].into(),
        terminations: vec![termination].into(),
        waits: vec![Ok(ExitWait::TimedOut), wait, Ok(ExitWait::Exited)].into(),
        ..FakeProcessScript::default()
    }
}

#[test]
fn batch_execution_reports_success_with_exact_read_only_summary() {
    let pid = 900;
    let creation_time = 90_000;
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![Ok(candidate(pid, "root.exe", None))])
        .with_process(
            pid,
            batch_execution_force_script(
                creation_time,
                Liveness::Alive,
                Ok(()),
                Ok(()),
                Ok(ExitWait::Exited),
            ),
        );
    let authority = authority(adapter.clone());
    let plan = authority
        .plan_batch(
            &target(pid, creation_time),
            ProcessBatchScope::Tree,
            ProcessBatchOperation::ForceTerminate,
        )
        .unwrap();

    let report = authority
        .execute_batch_with_clock(plan, &FakeMonotonicClock::at(Instant::now()))
        .unwrap();

    assert_eq!(report.scope(), ProcessBatchScope::Tree);
    assert_eq!(report.operation(), ProcessBatchOperation::ForceTerminate);
    assert_eq!(report.results().len(), 1);
    assert_eq!(report.results()[0].preview().pid(), pid);
    assert_eq!(
        report.results()[0].outcome(),
        ProcessBatchTargetOutcome::ObservedExited
    );
    assert_eq!(report.observed_exited(), 1);
    assert_eq!(report.already_exited(), 0);
    assert_eq!(report.failed(), 0);
    assert_eq!(report.not_attempted(), 0);
    assert_eq!(report.completion(), ProcessBatchCompletion::Complete);
    assert_eq!(adapter.terminate_calls(), 1);
    assert_eq!(adapter.opened(), adapter.dropped());
}

#[test]
fn batch_execution_expired_plan_is_immutable_preflight() {
    let pid = 901;
    let creation_time = 90_100;
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![Ok(candidate(pid, "root.exe", None))])
        .with_process(
            pid,
            planning_script(creation_time, "root.exe", r"C:\Fixtures\root.exe"),
        );
    let plan = authority(adapter.clone())
        .plan_batch(
            &target(pid, creation_time),
            ProcessBatchScope::Tree,
            ProcessBatchOperation::Terminate,
        )
        .unwrap();
    let calls_before = adapter.calls();
    let other_host = HostInstanceId::new([0x42; 16]).unwrap();
    let other_self = ProcessTarget::new(other_host, SELF_PID + 1, SELF_CREATION_TIME + 1).unwrap();
    let other = CoreAuthority::new(other_host, other_self, adapter.clone());

    assert!(matches!(
        other.execute_batch_with_clock(plan, &FakeMonotonicClock::at(Instant::now())),
        Err(ProcessBatchError::ExpiredPlan)
    ));
    assert_eq!(adapter.calls(), calls_before);
    assert_eq!(adapter.opened(), adapter.dropped());
}

#[test]
fn batch_execution_deadline_before_admission_marks_every_target_once() {
    let pid = 902;
    let creation_time = 90_200;
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![Ok(candidate(pid, "root.exe", None))])
        .with_process(
            pid,
            planning_script(creation_time, "root.exe", r"C:\Fixtures\root.exe"),
        );
    let authority = authority(adapter.clone());
    let plan = authority
        .plan_batch(
            &target(pid, creation_time),
            ProcessBatchScope::Tree,
            ProcessBatchOperation::Terminate,
        )
        .unwrap();
    let base = Instant::now();
    let clock =
        FakeMonotonicClock::with_readings(base, vec![Duration::ZERO, Duration::from_secs(30)]);

    let report = authority.execute_batch_with_clock(plan, &clock).unwrap();

    assert_eq!(
        report.results()[0].outcome(),
        ProcessBatchTargetOutcome::NotAttemptedBatchDeadline
    );
    assert_eq!(report.not_attempted(), 1);
    assert_eq!(report.completion(), ProcessBatchCompletion::Partial);
    assert_eq!(adapter.requested_operation_access(), vec![]);
}

#[test]
fn batch_execution_target_errors_are_stable_and_redacted() {
    let errors = [
        ProcessBatchTargetError::TargetChanged,
        ProcessBatchTargetError::InspectionDenied,
        ProcessBatchTargetError::ProtectedTarget,
        ProcessBatchTargetError::OperationAccessDenied,
        ProcessBatchTargetError::GracefulCloseUnavailable,
        ProcessBatchTargetError::GracefulCloseTimedOut,
        ProcessBatchTargetError::OperationUnavailable,
        ProcessBatchTargetError::TerminationPending,
        ProcessBatchTargetError::CompletionUnknown,
    ];
    for error in errors {
        assert_eq!(error.to_string(), format!("{error:?}"));
        assert!(!error.to_string().contains("secret"));
    }
}

#[test]
fn batch_execution_deadline_after_successful_admission_closes_it_and_stops() {
    let pid = 903;
    let creation_time = 90_300;
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![Ok(candidate(pid, "root.exe", None))])
        .with_process(
            pid,
            batch_execution_force_script(
                creation_time,
                Liveness::Alive,
                Ok(()),
                Ok(()),
                Ok(ExitWait::Exited),
            ),
        );
    let authority = authority(adapter.clone());
    let plan = authority
        .plan_batch(
            &target(pid, creation_time),
            ProcessBatchScope::Tree,
            ProcessBatchOperation::ForceTerminate,
        )
        .unwrap();
    let opened_before = adapter.opened();
    let base = Instant::now();
    let clock = FakeMonotonicClock::with_readings(
        base,
        vec![Duration::ZERO, Duration::ZERO, Duration::from_secs(30)],
    );

    let report = authority.execute_batch_with_clock(plan, &clock).unwrap();

    assert_eq!(
        report.results()[0].outcome(),
        ProcessBatchTargetOutcome::NotAttemptedBatchDeadline
    );
    assert_eq!(adapter.opened(), opened_before + 1);
    assert_eq!(adapter.opened(), adapter.dropped());
    assert!(adapter.requested_operation_access().is_empty());
}

#[test]
fn batch_execution_failed_admission_is_kept_when_next_deadline_check_expires() {
    let root_pid = 904;
    let child_pid = 905;
    let (adapter, plan) = prepared_tree(
        root_pid,
        90_400,
        vec![candidate(child_pid, "child.exe", Some(root_pid))],
        vec![(
            child_pid,
            successful_script(90_500, "child.exe", r"C:\Fixtures\child.exe"),
        )],
    );
    adapter.replace_process(
        child_pid,
        FakeProcessScript {
            open: vec![Ok(())].into(),
            creation_times: vec![Ok(99_999)].into(),
            ..FakeProcessScript::default()
        },
    );
    let base = Instant::now();
    let clock = FakeMonotonicClock::with_readings(
        base,
        vec![Duration::ZERO, Duration::ZERO, Duration::from_secs(30)],
    );

    let report = authority(adapter.clone())
        .execute_batch_with_clock(plan, &clock)
        .unwrap();

    assert_eq!(
        report
            .results()
            .iter()
            .map(ProcessBatchTargetResult::outcome)
            .collect::<Vec<_>>(),
        vec![
            ProcessBatchTargetOutcome::Failed(ProcessBatchTargetError::TargetChanged),
            ProcessBatchTargetOutcome::NotAttemptedBatchDeadline,
        ]
    );
    assert_eq!(report.failed(), 1);
    assert_eq!(report.not_attempted(), 1);
}

#[test]
fn batch_execution_continues_to_root_and_stage_two_keeps_full_five_seconds() {
    let root_pid = 906;
    let child_pid = 907;
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![
            Ok(candidate(root_pid, "root.exe", None)),
            Ok(candidate(child_pid, "child.exe", Some(root_pid))),
        ])
        .with_process(
            root_pid,
            planning_script(90_600, "root.exe", r"C:\Fixtures\root.exe"),
        )
        .with_process(
            child_pid,
            successful_script(90_700, "child.exe", r"C:\Fixtures\child.exe"),
        );
    let prepared = authority(adapter.clone())
        .prepare_batch_planning(
            &target(root_pid, 90_600),
            ProcessBatchOperation::ForceTerminate,
        )
        .unwrap();
    let plan = build_tree_plan(prepared).unwrap();
    adapter.replace_process(
        child_pid,
        FakeProcessScript {
            open: vec![Ok(())].into(),
            creation_times: vec![Ok(90_700)].into(),
            liveness: vec![Ok(Liveness::Exited)].into(),
            protections: vec![Ok(ProcessProtection::new(false, false))].into(),
            ..FakeProcessScript::default()
        },
    );
    adapter.replace_process(
        root_pid,
        batch_execution_force_script(
            90_600,
            Liveness::Alive,
            Ok(()),
            Ok(()),
            Ok(ExitWait::Exited),
        ),
    );

    let report = authority(adapter.clone())
        .execute_batch_with_clock(plan, &FakeMonotonicClock::at(Instant::now()))
        .unwrap();

    assert_eq!(
        report
            .results()
            .iter()
            .map(ProcessBatchTargetResult::outcome)
            .collect::<Vec<_>>(),
        vec![
            ProcessBatchTargetOutcome::AlreadyExited,
            ProcessBatchTargetOutcome::ObservedExited
        ]
    );
    assert!(adapter
        .calls()
        .contains(&FakeCall::WaitForExit(root_pid, Duration::from_secs(5))));
    assert_eq!(report.completion(), ProcessBatchCompletion::Complete);
}

#[test]
fn batch_execution_normalizes_every_stage_error_without_top_level_escape() {
    let admission_cases = [
        (
            ProcessError::InvalidTarget,
            ProcessBatchTargetError::OperationUnavailable,
        ),
        (
            ProcessError::ExpiredTarget,
            ProcessBatchTargetError::OperationUnavailable,
        ),
        (
            ProcessError::TargetChanged,
            ProcessBatchTargetError::TargetChanged,
        ),
        (
            ProcessError::InspectionDenied,
            ProcessBatchTargetError::InspectionDenied,
        ),
        (
            ProcessError::ProtectedTarget,
            ProcessBatchTargetError::ProtectedTarget,
        ),
        (
            ProcessError::CatalogUnavailable,
            ProcessBatchTargetError::OperationUnavailable,
        ),
        (
            ProcessError::PlatformUnsupported,
            ProcessBatchTargetError::OperationUnavailable,
        ),
    ];
    for (source, expected) in admission_cases {
        assert_eq!(
            crate::process_control::batch::map_admission_outcome(source),
            ProcessBatchTargetOutcome::Failed(expected)
        );
    }
    assert_eq!(
        crate::process_control::batch::map_admission_outcome(ProcessError::TargetExited),
        ProcessBatchTargetOutcome::AlreadyExited
    );

    let operation_cases = [
        (
            ProcessOperationError::IntentMismatch,
            ProcessBatchTargetError::OperationUnavailable,
        ),
        (
            ProcessOperationError::TargetChanged,
            ProcessBatchTargetError::TargetChanged,
        ),
        (
            ProcessOperationError::InspectionDenied,
            ProcessBatchTargetError::InspectionDenied,
        ),
        (
            ProcessOperationError::ProtectedTarget,
            ProcessBatchTargetError::ProtectedTarget,
        ),
        (
            ProcessOperationError::OperationAccessDenied,
            ProcessBatchTargetError::OperationAccessDenied,
        ),
        (
            ProcessOperationError::GracefulCloseUnavailable,
            ProcessBatchTargetError::GracefulCloseUnavailable,
        ),
        (
            ProcessOperationError::GracefulCloseTimedOut,
            ProcessBatchTargetError::GracefulCloseTimedOut,
        ),
        (
            ProcessOperationError::OperationUnavailable,
            ProcessBatchTargetError::OperationUnavailable,
        ),
        (
            ProcessOperationError::TerminationPending,
            ProcessBatchTargetError::TerminationPending,
        ),
        (
            ProcessOperationError::CompletionUnknown,
            ProcessBatchTargetError::CompletionUnknown,
        ),
        (
            ProcessOperationError::PlatformUnsupported,
            ProcessBatchTargetError::OperationUnavailable,
        ),
    ];
    for (source, expected) in operation_cases {
        assert_eq!(
            crate::process_control::batch::map_operation_outcome(source),
            ProcessBatchTargetOutcome::Failed(expected)
        );
    }
    assert_eq!(
        crate::process_control::batch::map_operation_outcome(ProcessOperationError::TargetExited),
        ProcessBatchTargetOutcome::AlreadyExited
    );
}

#[test]
fn batch_execution_stage_two_target_exited_is_already_exited_without_retry() {
    let pid = 908;
    let creation_time = 90_800;
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![Ok(candidate(pid, "root.exe", None))])
        .with_process(
            pid,
            FakeProcessScript {
                open: vec![Ok(()), Ok(()), Ok(())].into(),
                operation_open: vec![Ok(())].into(),
                creation_times: vec![Ok(creation_time); 4].into(),
                liveness: vec![
                    Ok(Liveness::Alive),
                    Ok(Liveness::Alive),
                    Ok(Liveness::Alive),
                    Ok(Liveness::Exited),
                ]
                .into(),
                observations: vec![Ok(path_observation("root.exe", r"C:\Fixtures\root.exe"))]
                    .into(),
                protections: vec![Ok(ProcessProtection::new(false, false)); 2].into(),
                ..FakeProcessScript::default()
            },
        );
    let authority = authority(adapter.clone());
    let plan = authority
        .plan_batch(
            &target(pid, creation_time),
            ProcessBatchScope::Tree,
            ProcessBatchOperation::ForceTerminate,
        )
        .unwrap();

    let report = authority
        .execute_batch_with_clock(plan, &FakeMonotonicClock::at(Instant::now()))
        .unwrap();

    assert_eq!(
        report.results()[0].outcome(),
        ProcessBatchTargetOutcome::AlreadyExited
    );
    assert_eq!(adapter.requested_operation_access().len(), 1);
    assert_eq!(adapter.terminate_calls(), 0);
    assert_eq!(adapter.opened(), adapter.dropped());
}

#[test]
fn batch_execution_unwind_drops_current_handles_exactly_once() {
    let pid = 909;
    let creation_time = 90_900;
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![Ok(candidate(pid, "root.exe", None))])
        .with_process(
            pid,
            batch_execution_force_script(
                creation_time,
                Liveness::Alive,
                Ok(()),
                Ok(()),
                Ok(ExitWait::Exited),
            ),
        )
        .mutate_after(
            FakeCall::TerminateProcess(pid),
            1,
            FakeMutation::ResumeUnwind("batch execution unwind fixture"),
        );
    let authority = authority(adapter.clone());
    let plan = authority
        .plan_batch(
            &target(pid, creation_time),
            ProcessBatchScope::Tree,
            ProcessBatchOperation::ForceTerminate,
        )
        .unwrap();

    let unwind = catch_unwind(AssertUnwindSafe(|| {
        let _ = authority.execute_batch_with_clock(plan, &FakeMonotonicClock::at(Instant::now()));
    }));

    assert!(unwind.is_err());
    assert_eq!(adapter.opened(), adapter.dropped());
    let dropped = adapter
        .handle_events()
        .into_iter()
        .filter_map(|event| match event {
            FakeHandleEvent::Dropped { handle_id, .. } => Some(handle_id),
            FakeHandleEvent::Opened { .. } => None,
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(dropped.len(), adapter.dropped());
}

#[test]
fn batch_execution_omissions_make_an_otherwise_successful_report_partial() {
    let root_pid = 910;
    let omitted_pid = 911;
    let creation_time = 91_000;
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![
            Ok(candidate(root_pid, "root.exe", None)),
            Ok(candidate(omitted_pid, "denied.exe", Some(root_pid))),
        ])
        .with_process(
            root_pid,
            planning_script(creation_time, "root.exe", r"C:\Fixtures\root.exe"),
        )
        .with_process(
            omitted_pid,
            FakeProcessScript {
                open: vec![Err(ObservationFailure::AccessDenied)].into(),
                ..FakeProcessScript::default()
            },
        );
    let prepared = authority(adapter.clone())
        .prepare_batch_planning(
            &target(root_pid, creation_time),
            ProcessBatchOperation::ForceTerminate,
        )
        .unwrap();
    let plan = build_tree_plan(prepared).unwrap();
    adapter.replace_process(
        root_pid,
        batch_execution_force_script(
            creation_time,
            Liveness::Alive,
            Ok(()),
            Ok(()),
            Ok(ExitWait::Exited),
        ),
    );

    let report = authority(adapter)
        .execute_batch_with_clock(plan, &FakeMonotonicClock::at(Instant::now()))
        .unwrap();

    assert_eq!(report.observed_exited(), 1);
    assert_eq!(report.omissions().inspection_denied(), 1);
    assert_eq!(report.completion(), ProcessBatchCompletion::Partial);
}

#[test]
fn batch_execution_overlapping_plans_execute_with_independent_admissions() {
    let pid = 912;
    let creation_time = 91_200;
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![Ok(candidate(pid, "root.exe", None))])
        .with_enumeration(vec![Ok(candidate(pid, "root.exe", None))])
        .with_process(
            pid,
            FakeProcessScript {
                open: vec![Ok(()); 4].into(),
                creation_times: vec![Ok(creation_time); 4].into(),
                liveness: vec![Ok(Liveness::Alive); 4].into(),
                observations: vec![
                    Ok(path_observation("root.exe", r"C:\Fixtures\root.exe")),
                    Ok(path_observation("root.exe", r"C:\Fixtures\root.exe")),
                ]
                .into(),
                protections: vec![Ok(ProcessProtection::new(false, false)); 2].into(),
                ..FakeProcessScript::default()
            },
        );
    let authority = authority(adapter.clone());
    let first = authority
        .plan_batch(
            &target(pid, creation_time),
            ProcessBatchScope::Tree,
            ProcessBatchOperation::ForceTerminate,
        )
        .unwrap();
    let second = authority
        .plan_batch(
            &target(pid, creation_time),
            ProcessBatchScope::Tree,
            ProcessBatchOperation::ForceTerminate,
        )
        .unwrap();
    adapter.replace_process(
        pid,
        FakeProcessScript {
            open: vec![Ok(()); 2].into(),
            operation_open: vec![Ok(()); 2].into(),
            creation_times: vec![Ok(creation_time); 4].into(),
            liveness: vec![Ok(Liveness::Alive); 4].into(),
            protections: vec![Ok(ProcessProtection::new(false, false)); 4].into(),
            terminations: vec![Ok(()); 2].into(),
            waits: vec![
                Ok(ExitWait::TimedOut),
                Ok(ExitWait::Exited),
                Ok(ExitWait::Exited),
                Ok(ExitWait::TimedOut),
                Ok(ExitWait::Exited),
                Ok(ExitWait::Exited),
            ]
            .into(),
            ..FakeProcessScript::default()
        },
    );
    let clock = FakeMonotonicClock::at(Instant::now());

    let first_report = authority.execute_batch_with_clock(first, &clock).unwrap();
    let second_report = authority.execute_batch_with_clock(second, &clock).unwrap();

    assert_eq!(first_report.completion(), ProcessBatchCompletion::Complete);
    assert_eq!(second_report.completion(), ProcessBatchCompletion::Complete);
    assert_eq!(adapter.requested_operation_access().len(), 2);
    assert_eq!(adapter.terminate_calls(), 2);
    assert_eq!(adapter.opened(), adapter.dropped());
}

#[test]
fn batch_execution_failed_tree_child_preserves_order_and_continues_without_fallback() {
    let root = (920, 92_000);
    let child = (921, 92_100);
    let grandchild = (922, 92_200);
    let adapter = FakeProcessAdapter::default()
        .with_enumeration(vec![
            Ok(candidate(root.0, "root.exe", None)),
            Ok(candidate(child.0, "child.exe", Some(root.0))),
            Ok(candidate(grandchild.0, "grandchild.exe", Some(child.0))),
        ])
        .with_process(
            root.0,
            planning_script(root.1, "root.exe", r"C:\Fixtures\root.exe"),
        )
        .with_process(
            child.0,
            successful_script(child.1, "child.exe", r"C:\Fixtures\child.exe"),
        )
        .with_process(
            grandchild.0,
            successful_script(
                grandchild.1,
                "grandchild.exe",
                r"C:\Fixtures\grandchild.exe",
            ),
        );
    let authority = authority(adapter.clone());
    let prepared = authority
        .prepare_batch_planning(
            &target(root.0, root.1),
            ProcessBatchOperation::ForceTerminate,
        )
        .unwrap();
    let plan = build_tree_plan(prepared).unwrap();
    adapter.replace_process(
        grandchild.0,
        FakeProcessScript {
            open: vec![Ok(())].into(),
            operation_open: vec![Err(OperationFailure::AccessDenied)].into(),
            creation_times: vec![Ok(grandchild.1)].into(),
            liveness: vec![Ok(Liveness::Alive)].into(),
            protections: vec![Ok(ProcessProtection::new(false, false))].into(),
            ..FakeProcessScript::default()
        },
    );
    adapter.replace_process(
        child.0,
        FakeProcessScript {
            open: vec![Ok(())].into(),
            operation_open: vec![Ok(())].into(),
            creation_times: vec![Ok(child.1), Ok(child.1)].into(),
            liveness: vec![Ok(Liveness::Alive), Ok(Liveness::Alive)].into(),
            protections: vec![Ok(ProcessProtection::new(false, false)); 2].into(),
            terminations: vec![Ok(())].into(),
            waits: vec![
                Ok(ExitWait::TimedOut),
                Ok(ExitWait::Exited),
                Ok(ExitWait::Exited),
            ]
            .into(),
            ..FakeProcessScript::default()
        },
    );
    adapter.replace_process(
        root.0,
        FakeProcessScript {
            open: vec![Ok(())].into(),
            creation_times: vec![Ok(root.1)].into(),
            liveness: vec![Ok(Liveness::Exited)].into(),
            ..FakeProcessScript::default()
        },
    );

    let report = authority
        .execute_batch_with_clock(plan, &FakeMonotonicClock::at(Instant::now()))
        .unwrap();

    assert_eq!(
        report
            .results()
            .iter()
            .map(|result| (result.preview().pid(), result.outcome()))
            .collect::<Vec<_>>(),
        vec![
            (
                grandchild.0,
                ProcessBatchTargetOutcome::Failed(ProcessBatchTargetError::OperationAccessDenied)
            ),
            (child.0, ProcessBatchTargetOutcome::ObservedExited),
            (root.0, ProcessBatchTargetOutcome::AlreadyExited),
        ]
    );
    assert_eq!(
        (
            report.observed_exited(),
            report.already_exited(),
            report.failed()
        ),
        (1, 1, 1)
    );
    assert_eq!(report.completion(), ProcessBatchCompletion::Partial);
    assert_eq!(
        adapter
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                FakeCall::OpenObservation(pid)
                    if [grandchild.0, child.0, root.0].contains(&pid) =>
                {
                    Some(pid)
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>(),
        vec![grandchild.0, child.0, root.0]
    );
    assert_eq!(adapter.requested_operation_access().len(), 2);
    assert_eq!(adapter.terminate_calls(), 1);
    assert_eq!(adapter.opened(), adapter.dropped());
}

#[test]
fn batch_execution_same_name_uses_pid_order_and_independent_admissions() {
    let pids = [(930, 93_000), (928, 92_800), (929, 92_900)];
    let mut adapter = FakeProcessAdapter::default().with_enumeration(
        pids.iter()
            .map(|(pid, _)| Ok(candidate(*pid, "same.exe", None)))
            .collect(),
    );
    for (pid, creation) in pids {
        adapter = adapter.with_process(
            pid,
            if pid == 930 {
                planning_script(creation, "same.exe", r"C:\Fixtures\same.exe")
            } else {
                successful_script(creation, "same.exe", r"C:\Fixtures\same.exe")
            },
        );
    }
    let authority = authority(adapter.clone());
    let plan = authority
        .plan_batch(
            &target(930, 93_000),
            ProcessBatchScope::SameName,
            ProcessBatchOperation::ForceTerminate,
        )
        .unwrap();
    for (pid, creation) in pids {
        adapter.replace_process(
            pid,
            FakeProcessScript {
                open: vec![Ok(())].into(),
                creation_times: vec![Ok(creation)].into(),
                liveness: vec![Ok(Liveness::Exited)].into(),
                ..FakeProcessScript::default()
            },
        );
    }

    let report = authority
        .execute_batch_with_clock(plan, &FakeMonotonicClock::at(Instant::now()))
        .unwrap();

    assert_eq!(
        report
            .results()
            .iter()
            .map(|r| r.preview().pid())
            .collect::<Vec<_>>(),
        vec![928, 929, 930]
    );
    assert!(report
        .results()
        .iter()
        .all(|r| r.outcome() == ProcessBatchTargetOutcome::AlreadyExited));
    assert_eq!(report.already_exited(), 3);
    assert!(adapter.requested_operation_access().is_empty());
    assert_eq!(adapter.opened(), adapter.dropped());
}

fn three_target_execution_plan() -> (FakeProcessAdapter, ProcessBatchPlan) {
    prepared_tree(
        940,
        94_000,
        vec![
            candidate(941, "first.exe", Some(940)),
            candidate(942, "second.exe", Some(940)),
        ],
        vec![
            (
                941,
                successful_script(94_100, "first.exe", r"C:\Fixtures\first.exe"),
            ),
            (
                942,
                successful_script(94_200, "second.exe", r"C:\Fixtures\second.exe"),
            ),
        ],
    )
}

#[test]
fn batch_execution_pre_admission_deadline_preserves_current_and_all_remaining() {
    let (adapter, plan) = three_target_execution_plan();
    let calls_before = adapter.calls().len();
    let base = Instant::now();
    let clock =
        FakeMonotonicClock::with_readings(base, vec![Duration::ZERO, Duration::from_secs(30)]);

    let report = authority(adapter.clone())
        .execute_batch_with_clock(plan, &clock)
        .unwrap();

    assert_eq!(
        report
            .results()
            .iter()
            .map(|result| (result.preview().pid(), result.outcome()))
            .collect::<Vec<_>>(),
        vec![
            (941, ProcessBatchTargetOutcome::NotAttemptedBatchDeadline),
            (942, ProcessBatchTargetOutcome::NotAttemptedBatchDeadline),
            (940, ProcessBatchTargetOutcome::NotAttemptedBatchDeadline),
        ]
    );
    assert_eq!(report.not_attempted(), 3);
    assert_eq!(adapter.calls().len(), calls_before);
}

#[test]
fn batch_execution_post_admission_deadline_closes_current_and_preserves_tail() {
    let (adapter, plan) = three_target_execution_plan();
    adapter.replace_process(
        941,
        FakeProcessScript {
            open: vec![Ok(())].into(),
            creation_times: vec![Ok(94_100)].into(),
            liveness: vec![Ok(Liveness::Alive)].into(),
            protections: vec![Ok(ProcessProtection::new(false, false))].into(),
            ..FakeProcessScript::default()
        },
    );
    let calls_before = adapter.calls().len();
    let opened_before = adapter.opened();
    let base = Instant::now();
    let clock = FakeMonotonicClock::with_readings(
        base,
        vec![Duration::ZERO, Duration::ZERO, Duration::from_secs(30)],
    );

    let report = authority(adapter.clone())
        .execute_batch_with_clock(plan, &clock)
        .unwrap();

    assert_eq!(report.results().len(), 3);
    assert!(report
        .results()
        .iter()
        .all(|result| result.outcome() == ProcessBatchTargetOutcome::NotAttemptedBatchDeadline));
    assert_eq!(adapter.opened(), opened_before + 1);
    assert_eq!(adapter.opened(), adapter.dropped());
    assert_eq!(
        adapter.calls()[calls_before..]
            .iter()
            .filter(|call| matches!(call, FakeCall::OpenObservation(_)))
            .count(),
        1
    );
    assert!(adapter.requested_operation_access().is_empty());
}

#[test]
fn batch_execution_graceful_failure_continues_without_force_fallback() {
    let (adapter, plan) = three_target_execution_plan();
    adapter.replace_process(
        941,
        FakeProcessScript {
            open: vec![Ok(())].into(),
            operation_open: vec![Err(OperationFailure::Unavailable)].into(),
            creation_times: vec![Ok(94_100)].into(),
            liveness: vec![Ok(Liveness::Alive)].into(),
            protections: vec![Ok(ProcessProtection::new(false, false))].into(),
            ..FakeProcessScript::default()
        },
    );
    for (pid, creation) in [(942, 94_200), (940, 94_000)] {
        adapter.replace_process(
            pid,
            FakeProcessScript {
                open: vec![Ok(())].into(),
                creation_times: vec![Ok(creation)].into(),
                liveness: vec![Ok(Liveness::Exited)].into(),
                ..FakeProcessScript::default()
            },
        );
    }

    let report = authority(adapter.clone())
        .execute_batch_with_clock(plan, &FakeMonotonicClock::at(Instant::now()))
        .unwrap();

    assert_eq!(
        report
            .results()
            .iter()
            .map(ProcessBatchTargetResult::outcome)
            .collect::<Vec<_>>(),
        vec![
            ProcessBatchTargetOutcome::Failed(ProcessBatchTargetError::OperationUnavailable),
            ProcessBatchTargetOutcome::AlreadyExited,
            ProcessBatchTargetOutcome::AlreadyExited,
        ]
    );
    assert_eq!(
        adapter.requested_operation_access(),
        vec![OperationAccess::GRACEFUL]
    );
    assert_eq!(adapter.terminate_calls(), 0);
    assert_eq!(adapter.opened(), adapter.dropped());
}

#[cfg(windows)]
mod windows_batch {
    use super::*;
    use std::{
        fs,
        process::{Child, Command, Stdio},
        sync::atomic::{AtomicBool, AtomicU64, Ordering},
        thread,
    };
    use windows::{
        core::w,
        Win32::{
            Foundation::{
                CloseHandle, FILETIME, HANDLE, HWND, LPARAM, LRESULT, WAIT_FAILED, WAIT_OBJECT_0,
                WAIT_TIMEOUT, WPARAM,
            },
            System::Threading::{
                GetProcessTimes, OpenProcess, TerminateProcess, WaitForSingleObject,
                PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
            },
            UI::WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
                PostQuitMessage, RegisterClassW, ShowWindow, TranslateMessage, CW_USEDEFAULT, MSG,
                SW_SHOWNOACTIVATE, WM_CLOSE, WM_DESTROY, WNDCLASSW, WS_EX_NOACTIVATE,
                WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW,
            },
        },
    };

    use crate::process_control::{ProcessAuthority, ProcessTarget};

    const FIXTURE_TEST: &str =
        "process_control::tests::batch::windows_batch::windows_batch_fixture";
    const MARKER: &str = "UIPILOT_BATCH_FIXTURE";
    const MODE: &str = "UIPILOT_BATCH_FIXTURE_MODE";
    const CONTROL: &str = "UIPILOT_BATCH_FIXTURE_CONTROL";
    const TREE_KIND: &str = "UIPILOT_BATCH_FIXTURE_TREE_KIND";
    const WAIT_MS: u32 = 5_000;
    static UNIQUE: AtomicU64 = AtomicU64::new(0);
    static IGNORE_CLOSE: AtomicBool = AtomicBool::new(false);

    struct CleanupHandle(HANDLE);

    impl Drop for CleanupHandle {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    struct OwnedNode {
        pid: u32,
        creation_time: u64,
        handle: CleanupHandle,
        child: Option<Child>,
    }

    struct PendingChild(Option<Child>);

    impl PendingChild {
        fn new(child: Child) -> Self {
            Self(Some(child))
        }

        fn id(&self) -> u32 {
            self.0.as_ref().unwrap().id()
        }

        fn transfer(mut self) -> Child {
            self.0.take().unwrap()
        }

        fn disarm(mut self) {
            let _ = self.0.take();
        }
    }

    impl Drop for PendingChild {
        fn drop(&mut self) {
            if let Some(child) = self.0.as_mut() {
                let _ = child.kill();
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    match child.try_wait() {
                        Ok(Some(_)) | Err(_) => break,
                        Ok(None) if Instant::now() < deadline => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Ok(None) => break,
                    }
                }
            }
        }
    }

    struct FixtureController {
        authority: ProcessAuthority,
        dir: PathBuf,
        copied_exe: Option<PathBuf>,
        nodes: Vec<OwnedNode>,
    }

    impl FixtureController {
        fn new(copy_exe: bool) -> Self {
            let nonce = UNIQUE.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "uipilot-batch-{}-{}-{nonce}",
                std::process::id(),
                Instant::now().elapsed().as_nanos()
            ));
            fs::create_dir(&dir).unwrap();
            let copied_exe = copy_exe.then(|| {
                let path = dir.join(format!("uib{:x}{:x}.exe", std::process::id(), nonce));
                fs::copy(std::env::current_exe().unwrap(), &path).unwrap();
                path
            });
            Self {
                authority: ProcessAuthority::new().unwrap(),
                dir,
                copied_exe,
                nodes: Vec::new(),
            }
        }

        fn spawn(&mut self, mode: &str) -> usize {
            let exe = self
                .copied_exe
                .clone()
                .unwrap_or_else(|| std::env::current_exe().unwrap());
            let node = self.spawn_exe(&exe, mode, None, true);
            node
        }

        fn spawn_alternate_path(&mut self, mode: &str) -> usize {
            let primary = self.copied_exe.as_ref().unwrap();
            let alternate_dir = self.dir.join("alternate");
            fs::create_dir(&alternate_dir).unwrap();
            let alternate = alternate_dir.join(primary.file_name().unwrap());
            if !alternate.exists() {
                fs::copy(std::env::current_exe().unwrap(), &alternate).unwrap();
            }
            self.spawn_exe(&alternate, mode, None, true)
        }

        fn spawn_select_window(&mut self) -> usize {
            let exe = self.copied_exe.clone().unwrap();
            self.spawn_exe(&exe, "select-window", None, false)
        }

        fn select_window_mode(&self, node: usize, mode: &str) {
            fs::write(
                self.dir.join(format!("{}.mode", self.nodes[node].pid)),
                mode,
            )
            .unwrap();
            self.wait_ready(node);
        }

        fn spawn_tree(&mut self, kind: &str) -> [usize; 3] {
            let exe = std::env::current_exe().unwrap();
            let root = self.spawn_exe(&exe, "tree-parent", Some(kind), false);
            let child_pid = self.read_pid("child.pid");
            let child = self.retain_pid(child_pid, None);
            fs::write(self.dir.join("child.ack"), b"retained").unwrap();
            let grandchild_pid = self.read_pid("grandchild.pid");
            let grandchild = self.retain_pid(grandchild_pid, None);
            fs::write(self.dir.join("grandchild.ack"), b"retained").unwrap();
            for node in [root, child, grandchild] {
                self.wait_ready(node);
            }
            [root, child, grandchild]
        }

        fn spawn_exe(
            &mut self,
            exe: &std::path::Path,
            mode: &str,
            kind: Option<&str>,
            wait_ready: bool,
        ) -> usize {
            let mut command = Command::new(exe);
            command
                .args(["--ignored", "--exact", FIXTURE_TEST, "--test-threads=1"])
                .env(MARKER, "1")
                .env(MODE, mode)
                .env(CONTROL, &self.dir)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            if let Some(kind) = kind {
                command.env(TREE_KIND, kind);
            }
            let child = PendingChild::new(command.spawn().unwrap());
            let pid = child.id();
            let record = wait_record(&self.authority, pid, Duration::from_secs(5));
            let node = self.retain_record(record, Some(child.transfer()));
            if wait_ready {
                self.wait_ready(node);
            }
            node
        }

        fn retain_pid(&mut self, pid: u32, child: Option<Child>) -> usize {
            let record = wait_record(&self.authority, pid, Duration::from_secs(5));
            self.retain_record(record, child)
        }

        fn retain_record(
            &mut self,
            record: crate::process_control::ProcessRecord,
            child: Option<Child>,
        ) -> usize {
            let pid = record.pid();
            let handle = unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | PROCESS_TERMINATE,
                    false,
                    pid,
                )
            }
            .unwrap();
            let handle = CleanupHandle(handle);
            let creation_time = creation_time(handle.0).unwrap();
            assert_eq!(creation_time, record.target().creation_time());
            self.nodes.push(OwnedNode {
                pid,
                creation_time,
                handle,
                child,
            });
            self.nodes.len() - 1
        }

        fn target(&self, node: usize) -> ProcessTarget {
            wait_record(
                &self.authority,
                self.nodes[node].pid,
                Duration::from_secs(5),
            )
            .target()
            .clone()
        }

        fn read_pid(&self, name: &str) -> u32 {
            let path = self.dir.join(name);
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Ok(value) = fs::read_to_string(&path) {
                    return value.trim().parse().unwrap();
                }
                assert!(Instant::now() < deadline, "fixture handshake timed out");
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn wait_ready(&self, node: usize) {
            let path = self.dir.join(format!("{}.ready", self.nodes[node].pid));
            let deadline = Instant::now() + Duration::from_secs(5);
            while !path.exists() {
                assert!(Instant::now() < deadline, "fixture readiness timed out");
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn assert_all_name_matches_owned(&self, root: usize) {
            let snapshot = self.authority.snapshot().unwrap();
            let root_name = snapshot
                .processes()
                .iter()
                .find(|record| record.pid() == self.nodes[root].pid)
                .unwrap()
                .name()
                .to_owned();
            let matching: Vec<_> = snapshot
                .processes()
                .iter()
                .filter(|record| record.name().eq_ignore_ascii_case(&root_name))
                .collect();
            assert!(!matching.is_empty());
            assert!(
                matching.iter().all(|record| self.nodes.iter().any(|node| {
                    node.pid == record.pid()
                        && node.creation_time == record.target().creation_time()
                })),
                "unowned reliable same-name process; aborting before effects"
            );
        }

        fn assert_plan_owned(&self, plan: &ProcessBatchPlan, expected: &[usize]) {
            let mut expected_pids = expected
                .iter()
                .map(|&node| self.nodes[node].pid)
                .collect::<Vec<_>>();
            expected_pids.sort_unstable();
            let mut planned_pids = plan.previews().iter().map(|p| p.pid()).collect::<Vec<_>>();
            planned_pids.sort_unstable();
            assert_eq!(planned_pids, expected_pids);
            let snapshot = self.authority.snapshot().unwrap();
            for &node in expected {
                let record = snapshot
                    .processes()
                    .iter()
                    .find(|record| record.pid() == self.nodes[node].pid)
                    .expect("owned planned fixture disappeared before execution");
                assert_eq!(
                    record.target().creation_time(),
                    self.nodes[node].creation_time
                );
                assert_eq!(
                    creation_time(self.nodes[node].handle.0).unwrap(),
                    self.nodes[node].creation_time
                );
            }
        }

        fn alive(&mut self, node: usize) -> bool {
            match self.nodes[node].child.as_mut() {
                Some(child) => matches!(child.try_wait(), Ok(None)),
                None => match unsafe { WaitForSingleObject(self.nodes[node].handle.0, 0) } {
                    WAIT_TIMEOUT => true,
                    WAIT_OBJECT_0 => false,
                    WAIT_FAILED => panic!("fixture wait failed"),
                    result => panic!("unexpected fixture wait result {result:?}"),
                },
            }
        }

        fn wait_dead(&mut self, node: usize) {
            let deadline = Instant::now() + Duration::from_secs(5);
            while self.alive(node) {
                assert!(Instant::now() < deadline, "owned fixture did not exit");
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn cleanup_asserted(&mut self) {
            self.cleanup(false);
            assert!(self.nodes.is_empty());
            assert!(!self.dir.exists(), "fixture temporary directory remains");
        }

        fn cleanup(&mut self, best_effort: bool) {
            let mut clean = true;
            for node in self.nodes.iter_mut().rev() {
                let initial = unsafe { WaitForSingleObject(node.handle.0, 0) };
                if initial == WAIT_TIMEOUT {
                    if unsafe { TerminateProcess(node.handle.0, 0x55) }.is_err() {
                        clean = false;
                    }
                    if unsafe { WaitForSingleObject(node.handle.0, WAIT_MS) } != WAIT_OBJECT_0 {
                        clean = false;
                    }
                } else if initial != WAIT_OBJECT_0 {
                    clean = false;
                }
                if let Some(child) = node.child.as_mut() {
                    let _ = child.try_wait();
                }
            }
            self.nodes.clear();
            if let Some(exe) = self.copied_exe.take() {
                if exe.exists() && fs::remove_file(exe).is_err() {
                    clean = false;
                }
            }
            if self.dir.exists() && fs::remove_dir_all(&self.dir).is_err() {
                clean = false;
            }
            if !best_effort {
                assert!(clean, "fixture cleanup did not reach zero residue");
            }
        }
    }

    impl Drop for FixtureController {
        fn drop(&mut self) {
            self.cleanup(true);
        }
    }

    fn creation_time(handle: HANDLE) -> windows::core::Result<u64> {
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user)? };
        Ok(((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64)
    }

    fn wait_record(
        authority: &ProcessAuthority,
        pid: u32,
        timeout: Duration,
    ) -> crate::process_control::ProcessRecord {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(record) = authority
                .snapshot()
                .unwrap()
                .processes()
                .iter()
                .find(|record| record.pid() == pid)
            {
                return record.clone();
            }
            assert!(Instant::now() < deadline, "fixture did not enter snapshot");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn windows_batch_tree_graceful_is_deepest_first() {
        let mut fixture = FixtureController::new(false);
        let [root, child, grandchild] = fixture.spawn_tree("graceful-window");
        let plan = fixture
            .authority
            .plan_batch(
                &fixture.target(root),
                ProcessBatchScope::Tree,
                ProcessBatchOperation::Terminate,
            )
            .unwrap();
        fixture.assert_plan_owned(&plan, &[root, child, grandchild]);
        let report = fixture.authority.execute_batch(plan).unwrap();
        assert_eq!(
            report
                .results()
                .iter()
                .map(|r| r.preview().pid())
                .collect::<Vec<_>>(),
            vec![
                fixture.nodes[grandchild].pid,
                fixture.nodes[child].pid,
                fixture.nodes[root].pid
            ]
        );
        assert!(report
            .results()
            .iter()
            .all(|r| r.outcome() == ProcessBatchTargetOutcome::ObservedExited));
        for node in [root, child, grandchild] {
            fixture.wait_dead(node);
        }
        fixture.cleanup_asserted();
    }

    #[test]
    fn windows_batch_tree_force_is_deepest_first() {
        let mut fixture = FixtureController::new(false);
        let [root, child, grandchild] = fixture.spawn_tree("sleep");
        let plan = fixture
            .authority
            .plan_batch(
                &fixture.target(root),
                ProcessBatchScope::Tree,
                ProcessBatchOperation::ForceTerminate,
            )
            .unwrap();
        fixture.assert_plan_owned(&plan, &[root, child, grandchild]);
        let report = fixture.authority.execute_batch(plan).unwrap();
        assert_eq!(
            report
                .results()
                .iter()
                .map(|r| r.preview().pid())
                .collect::<Vec<_>>(),
            vec![
                fixture.nodes[grandchild].pid,
                fixture.nodes[child].pid,
                fixture.nodes[root].pid
            ]
        );
        assert!(report
            .results()
            .iter()
            .all(|r| r.outcome() == ProcessBatchTargetOutcome::ObservedExited));
        for node in [root, child, grandchild] {
            fixture.wait_dead(node);
        }
        fixture.cleanup_asserted();
    }

    fn same_name_newcomer(operation: ProcessBatchOperation, modes: [&str; 2]) {
        let mut fixture = FixtureController::new(true);
        let first = fixture.spawn(modes[0]);
        let second = fixture.spawn_alternate_path(modes[1]);
        fixture.assert_all_name_matches_owned(first);
        let plan = fixture
            .authority
            .plan_batch(
                &fixture.target(first),
                ProcessBatchScope::SameName,
                operation,
            )
            .unwrap();
        let planned_paths = plan
            .previews()
            .iter()
            .map(|preview| preview.executable_path().unwrap().to_path_buf())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            planned_paths.len(),
            2,
            "same-name fixtures must use distinct paths"
        );
        let newcomer = fixture.spawn("sleep");
        fixture.assert_plan_owned(&plan, &[first, second]);
        fixture.assert_all_name_matches_owned(first);
        let report = fixture.authority.execute_batch(plan).unwrap();
        assert_eq!(report.results().len(), 2);
        assert!(!report
            .results()
            .iter()
            .any(|r| r.preview().pid() == fixture.nodes[newcomer].pid));
        assert!(fixture.alive(newcomer));
        fixture.wait_dead(first);
        fixture.wait_dead(second);
        fixture.cleanup_asserted();
    }

    #[test]
    fn windows_batch_same_name_graceful_freezes_before_newcomer() {
        same_name_newcomer(
            ProcessBatchOperation::Terminate,
            ["graceful-window", "graceful-window"],
        );
    }

    #[test]
    fn windows_batch_same_name_force_freezes_before_newcomer() {
        same_name_newcomer(ProcessBatchOperation::ForceTerminate, ["sleep", "sleep"]);
    }

    #[test]
    fn windows_batch_graceful_timeout_keeps_later_target_running() {
        let mut fixture = FixtureController::new(true);
        let first = fixture.spawn_select_window();
        let second = fixture.spawn_select_window();
        let (ignored, closed) = if fixture.nodes[first].pid < fixture.nodes[second].pid {
            (first, second)
        } else {
            (second, first)
        };
        fixture.select_window_mode(ignored, "ignore-close-window");
        fixture.select_window_mode(closed, "graceful-window");
        fixture.assert_all_name_matches_owned(ignored);
        let plan = fixture
            .authority
            .plan_batch(
                &fixture.target(ignored),
                ProcessBatchScope::SameName,
                ProcessBatchOperation::Terminate,
            )
            .unwrap();
        fixture.assert_plan_owned(&plan, &[ignored, closed]);
        fixture.assert_all_name_matches_owned(ignored);
        let report = fixture.authority.execute_batch(plan).unwrap();
        assert_eq!(
            report.results()[0].outcome(),
            ProcessBatchTargetOutcome::Failed(ProcessBatchTargetError::GracefulCloseTimedOut)
        );
        let redacted = ProcessBatchTargetError::GracefulCloseTimedOut.to_string();
        assert_eq!(redacted, "GracefulCloseTimedOut");
        assert!(!redacted.contains(&fixture.nodes[ignored].pid.to_string()));
        assert!(!redacted.contains(
            fixture
                .copied_exe
                .as_ref()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .as_ref()
        ));
        assert_eq!(
            report.results()[1].outcome(),
            ProcessBatchTargetOutcome::ObservedExited
        );
        assert!(fixture.alive(ignored));
        fixture.wait_dead(closed);
        fixture.cleanup_asserted();
    }

    #[test]
    fn windows_batch_source_uses_only_existing_stage_methods_and_forbids_expansion() {
        let source = include_str!("../batch.rs");
        for required in [
            ".admit(",
            "terminate_with_clock",
            "force_terminate_with_clock",
        ] {
            assert!(source.contains(required), "missing required {required}");
        }
        for forbidden in [
            "TerminateProcess",
            "PostMessageW",
            "ShellExecute",
            "std::process::Command",
            "taskkill",
            "PowerShell",
            "powershell",
            "PROCESS_ALL_ACCESS",
            "PROCESS_CREATE_THREAD",
            "Restart",
            "elevat",
            "tauri",
            "persist",
            "plugin",
        ] {
            assert!(!source.contains(forbidden), "found forbidden {forbidden}");
        }
    }

    #[test]
    #[ignore = "spawned explicitly by Windows batch fixture controllers"]
    fn windows_batch_fixture() {
        assert_eq!(std::env::var(MARKER).as_deref(), Ok("1"));
        thread::spawn(|| {
            thread::sleep(Duration::from_secs(20));
            std::process::exit(0x56);
        });
        match std::env::var(MODE).as_deref() {
            Ok("sleep") => {
                publish_ready();
                thread::sleep(Duration::from_secs(20));
            }
            Ok("graceful-window") => run_window(false),
            Ok("ignore-close-window") => run_window(true),
            Ok("select-window") => {
                let path = PathBuf::from(std::env::var_os(CONTROL).unwrap())
                    .join(format!("{}.mode", std::process::id()));
                let deadline = Instant::now() + Duration::from_secs(5);
                let selected = loop {
                    if let Ok(value) = fs::read_to_string(&path) {
                        break value;
                    }
                    if Instant::now() >= deadline {
                        return;
                    }
                    thread::sleep(Duration::from_millis(10));
                };
                run_window(selected.trim() == "ignore-close-window");
            }
            Ok("tree-parent") => run_tree_parent(),
            Ok("tree-child") => run_tree_child(),
            mode => panic!("unsupported batch fixture mode {mode:?}"),
        }
    }

    fn spawn_descendant(mode: &str) -> Child {
        Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", FIXTURE_TEST, "--test-threads=1"])
            .env(MARKER, "1")
            .env(MODE, mode)
            .env(CONTROL, std::env::var_os(CONTROL).unwrap())
            .env(TREE_KIND, std::env::var_os(TREE_KIND).unwrap())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    fn run_tree_parent() {
        let child = PendingChild::new(spawn_descendant("tree-child"));
        let control = PathBuf::from(std::env::var_os(CONTROL).unwrap());
        fs::write(control.join("child.pid"), child.id().to_string()).unwrap();
        if !wait_for_ack(&control.join("child.ack")) {
            return;
        }
        child.disarm();
        run_fixture_kind();
    }

    fn run_tree_child() {
        let grandchild =
            PendingChild::new(spawn_descendant(std::env::var(TREE_KIND).unwrap().as_str()));
        let control = PathBuf::from(std::env::var_os(CONTROL).unwrap());
        fs::write(control.join("grandchild.pid"), grandchild.id().to_string()).unwrap();
        if !wait_for_ack(&control.join("grandchild.ack")) {
            return;
        }
        grandchild.disarm();
        run_fixture_kind();
    }

    fn wait_for_ack(path: &std::path::Path) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !path.exists() {
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
        true
    }

    fn run_fixture_kind() {
        match std::env::var(TREE_KIND).as_deref() {
            Ok("sleep") => {
                publish_ready();
                thread::sleep(Duration::from_secs(20));
            }
            Ok("graceful-window") => run_window(false),
            mode => panic!("unsupported tree fixture kind {mode:?}"),
        }
    }

    fn run_window(ignore: bool) {
        IGNORE_CLOSE.store(ignore, Ordering::Relaxed);
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            lpszClassName: w!("UiPilotBatchFixtureWindow"),
            ..Default::default()
        };
        assert_ne!(unsafe { RegisterClassW(&class) }, 0);
        let window = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                w!("UiPilotBatchFixtureWindow"),
                w!("UiPilot batch fixture"),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                240,
                120,
                None,
                None,
                None,
                None,
            )
        }
        .unwrap();
        unsafe {
            let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
        }
        publish_ready();
        let mut message = MSG::default();
        loop {
            let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
            assert_ne!(result, -1);
            if result == 0 {
                break;
            }
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    fn publish_ready() {
        let path = PathBuf::from(std::env::var_os(CONTROL).unwrap())
            .join(format!("{}.ready", std::process::id()));
        fs::write(path, b"ready").unwrap();
    }

    unsafe extern "system" fn window_proc(
        window: HWND,
        message: u32,
        w: WPARAM,
        l: LPARAM,
    ) -> LRESULT {
        match message {
            WM_CLOSE if IGNORE_CLOSE.load(Ordering::Relaxed) => LRESULT(0),
            WM_CLOSE => {
                let _ = unsafe { DestroyWindow(window) };
                LRESULT(0)
            }
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(window, message, w, l) },
        }
    }
}
