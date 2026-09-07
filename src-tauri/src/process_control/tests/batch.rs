use std::{
    collections::{BTreeMap, BTreeSet},
    panic::{catch_unwind, AssertUnwindSafe},
    path::PathBuf,
};

use syn::{
    visit::{self, Visit},
    ImplItem, TraitItem, Type, UseTree, Visibility,
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
    let syntax = syn::parse_file(source).expect("process_control source must parse");
    let mut surface = CallerSurface::default();
    surface.visit_file(&syntax);
    let sensitive = tainted_names(&surface.definitions, sensitive_roots, impl_owners);
    let connectors = tainted_names(&surface.definitions, impl_owners, &[]);
    let connected_traits = surface.connected_traits(&connectors);
    let leaks = surface
        .signatures
        .iter()
        .filter(|signature| {
            surface.signature_is_visible(signature)
                && intersects(&signature.types, &sensitive)
                && (connectors.is_empty()
                    || intersects(&signature.types, &connectors)
                    || intersects(&signature.owners, &connectors)
                    || intersects(&signature.owners, &connected_traits))
        })
        .map(|signature| signature.label.clone())
        .collect::<Vec<_>>();
    if leaks.is_empty() {
        Ok(())
    } else {
        Err(leaks)
    }
}

#[derive(Default)]
struct TypeNames(BTreeSet<String>);

impl<'ast> Visit<'ast> for TypeNames {
    fn visit_path_segment(&mut self, segment: &'ast syn::PathSegment) {
        self.0.insert(segment.ident.to_string());
        visit::visit_path_segment(self, segment);
    }
}

enum SignatureVisibility {
    Explicit(bool),
    Trait { name: String, self_type: String },
}

struct CallerSignature {
    label: String,
    types: BTreeSet<String>,
    owners: BTreeSet<String>,
    visibility: SignatureVisibility,
}

#[derive(Default)]
struct CallerSurface {
    definitions: BTreeMap<String, BTreeSet<String>>,
    types: BTreeMap<String, bool>,
    traits: BTreeMap<String, bool>,
    trait_impls: Vec<(String, String, BTreeSet<String>)>,
    signatures: Vec<CallerSignature>,
}

impl CallerSurface {
    fn define(&mut self, name: String, dependencies: BTreeSet<String>) {
        self.definitions
            .entry(name)
            .or_default()
            .extend(dependencies);
    }

    fn expose_type(&mut self, name: String, visible: bool) {
        self.types.insert(name.clone(), visible);
        self.signatures.push(CallerSignature {
            label: format!("type {name}"),
            types: BTreeSet::from([name]),
            owners: BTreeSet::new(),
            visibility: SignatureVisibility::Explicit(visible),
        });
    }

    fn trait_is_visible(&self, name: &str) -> bool {
        fn resolve(
            name: &str,
            surface: &CallerSurface,
            visited: &mut BTreeSet<String>,
        ) -> Option<bool> {
            if !visited.insert(name.to_owned()) {
                return None;
            }
            if let Some(visible) = surface.traits.get(name) {
                return Some(*visible);
            }
            surface.definitions.get(name).and_then(|dependencies| {
                dependencies
                    .iter()
                    .find_map(|dependency| resolve(dependency, surface, visited))
            })
        }
        resolve(name, self, &mut BTreeSet::new()).unwrap_or(true)
    }

    fn signature_is_visible(&self, signature: &CallerSignature) -> bool {
        match &signature.visibility {
            SignatureVisibility::Explicit(visible) => *visible,
            SignatureVisibility::Trait { name, self_type } => {
                self.trait_is_visible(name) && self.type_is_visible(self_type)
            }
        }
    }

    fn type_is_visible(&self, name: &str) -> bool {
        self.types.get(name).copied().unwrap_or(true)
    }

    fn connected_traits(&self, connectors: &BTreeSet<String>) -> BTreeSet<String> {
        let mut connected = BTreeSet::new();
        for (trait_name, self_type, owners) in &self.trait_impls {
            if self.trait_is_visible(trait_name)
                && self.type_is_visible(self_type)
                && intersects(owners, connectors)
            {
                collect_dependency_closure(trait_name, &self.definitions, &mut connected);
            }
        }
        connected
    }
}

impl<'ast> Visit<'ast> for CallerSurface {
    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        self.define(item.ident.to_string(), names_from_fields(&item.fields));
        self.expose_type(item.ident.to_string(), caller_visible(&item.vis));
    }

    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        let mut dependencies = BTreeSet::new();
        for variant in &item.variants {
            dependencies.extend(names_from_fields(&variant.fields));
        }
        self.define(item.ident.to_string(), dependencies);
        self.expose_type(item.ident.to_string(), caller_visible(&item.vis));
    }

    fn visit_item_union(&mut self, item: &'ast syn::ItemUnion) {
        let mut dependencies = TypeNames::default();
        for field in &item.fields.named {
            dependencies.visit_type(&field.ty);
        }
        self.define(item.ident.to_string(), dependencies.0);
        self.expose_type(item.ident.to_string(), caller_visible(&item.vis));
    }

    fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
        self.define(item.ident.to_string(), names_from_type(&item.ty));
        self.expose_type(item.ident.to_string(), caller_visible(&item.vis));
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        for (alias, original) in use_bindings(&item.tree) {
            self.define(alias.clone(), BTreeSet::from([original.clone()]));
            self.signatures.push(CallerSignature {
                label: format!("re-export {alias}"),
                types: BTreeSet::from([alias, original]),
                owners: BTreeSet::new(),
                visibility: SignatureVisibility::Explicit(caller_visible(&item.vis)),
            });
        }
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.signatures.push(CallerSignature {
            label: format!("function {}", item.sig.ident),
            types: names_from_signature(&item.sig),
            owners: BTreeSet::new(),
            visibility: SignatureVisibility::Explicit(caller_visible(&item.vis)),
        });
    }

    fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
        let trait_name = item.ident.to_string();
        let visible = caller_visible(&item.vis);
        self.traits.insert(trait_name.clone(), visible);
        let owners = BTreeSet::from([trait_name.clone()]);
        for trait_item in &item.items {
            let (label, types) = match trait_item {
                TraitItem::Fn(method) => (
                    format!("trait method {trait_name}::{}", method.sig.ident),
                    names_from_signature(&method.sig),
                ),
                TraitItem::Const(value) => (
                    format!("trait const {trait_name}::{}", value.ident),
                    names_from_type(&value.ty),
                ),
                TraitItem::Type(value) => {
                    let mut names = TypeNames::default();
                    for bound in &value.bounds {
                        names.visit_type_param_bound(bound);
                    }
                    if let Some((_, default)) = &value.default {
                        names.visit_type(default);
                    }
                    (format!("trait type {trait_name}::{}", value.ident), names.0)
                }
                _ => continue,
            };
            self.signatures.push(CallerSignature {
                label,
                types,
                owners: owners.clone(),
                visibility: SignatureVisibility::Explicit(visible),
            });
        }
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let owners = names_from_type(&item.self_ty);
        let self_type = principal_type_name(&item.self_ty).unwrap_or_default();
        let trait_name = item
            .trait_
            .as_ref()
            .and_then(|(_, path, _)| path.segments.last())
            .map(|segment| segment.ident.to_string());
        if let Some(name) = &trait_name {
            self.trait_impls
                .push((name.clone(), self_type.clone(), owners.clone()));
            let mut types = owners.clone();
            if let Some((_, path, _)) = &item.trait_ {
                types.extend(names_from_path(path));
            }
            self.signatures.push(CallerSignature {
                label: format!("trait impl {name}"),
                types,
                owners: owners.clone(),
                visibility: SignatureVisibility::Trait {
                    name: name.clone(),
                    self_type: self_type.clone(),
                },
            });
        }
        for impl_item in &item.items {
            let (label, types, visibility) = match impl_item {
                ImplItem::Fn(method) => (
                    format!("impl method {}", method.sig.ident),
                    names_from_signature(&method.sig),
                    trait_name.as_ref().map_or_else(
                        || SignatureVisibility::Explicit(caller_visible(&method.vis)),
                        |name| SignatureVisibility::Trait {
                            name: name.clone(),
                            self_type: self_type.clone(),
                        },
                    ),
                ),
                ImplItem::Const(value) => (
                    format!("impl const {}", value.ident),
                    names_from_type(&value.ty),
                    trait_name.as_ref().map_or_else(
                        || SignatureVisibility::Explicit(caller_visible(&value.vis)),
                        |name| SignatureVisibility::Trait {
                            name: name.clone(),
                            self_type: self_type.clone(),
                        },
                    ),
                ),
                ImplItem::Type(value) => (
                    format!("impl type {}", value.ident),
                    names_from_type(&value.ty),
                    trait_name.as_ref().map_or_else(
                        || SignatureVisibility::Explicit(caller_visible(&value.vis)),
                        |name| SignatureVisibility::Trait {
                            name: name.clone(),
                            self_type: self_type.clone(),
                        },
                    ),
                ),
                _ => continue,
            };
            self.signatures.push(CallerSignature {
                label,
                types,
                owners: owners.clone(),
                visibility,
            });
        }
    }

    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        self.signatures.push(CallerSignature {
            label: format!("const {}", item.ident),
            types: names_from_type(&item.ty),
            owners: BTreeSet::new(),
            visibility: SignatureVisibility::Explicit(caller_visible(&item.vis)),
        });
    }

    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        self.signatures.push(CallerSignature {
            label: format!("static {}", item.ident),
            types: names_from_type(&item.ty),
            owners: BTreeSet::new(),
            visibility: SignatureVisibility::Explicit(caller_visible(&item.vis)),
        });
    }
}

fn names_from_fields(fields: &syn::Fields) -> BTreeSet<String> {
    let mut names = TypeNames::default();
    for field in fields {
        names.visit_type(&field.ty);
    }
    names.0
}

fn names_from_type(value: &Type) -> BTreeSet<String> {
    let mut names = TypeNames::default();
    names.visit_type(value);
    names.0
}

fn principal_type_name(value: &Type) -> Option<String> {
    match value {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        Type::Group(group) => principal_type_name(&group.elem),
        Type::Paren(paren) => principal_type_name(&paren.elem),
        Type::Reference(reference) => principal_type_name(&reference.elem),
        _ => None,
    }
}

fn names_from_signature(signature: &syn::Signature) -> BTreeSet<String> {
    let mut names = TypeNames::default();
    names.visit_signature(signature);
    names.0
}

fn names_from_path(path: &syn::Path) -> BTreeSet<String> {
    let mut names = TypeNames::default();
    names.visit_path(path);
    names.0
}

fn use_bindings(tree: &UseTree) -> Vec<(String, String)> {
    fn walk(tree: &UseTree, prefix: &mut Vec<String>, bindings: &mut Vec<(String, String)>) {
        match tree {
            UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                walk(&path.tree, prefix, bindings);
                prefix.pop();
            }
            UseTree::Name(name) => {
                let name = name.ident.to_string();
                let original = if name == "self" {
                    prefix.last().cloned().unwrap_or(name)
                } else {
                    name
                };
                bindings.push((original.clone(), original));
            }
            UseTree::Rename(rename) => {
                let original = if rename.ident == "self" {
                    prefix
                        .last()
                        .cloned()
                        .unwrap_or_else(|| rename.ident.to_string())
                } else {
                    rename.ident.to_string()
                };
                bindings.push((rename.rename.to_string(), original));
            }
            UseTree::Group(group) => {
                for tree in &group.items {
                    walk(tree, prefix, bindings);
                }
            }
            UseTree::Glob(_) => {}
        }
    }
    let mut bindings = Vec::new();
    walk(tree, &mut Vec::new(), &mut bindings);
    bindings
}

fn tainted_names(
    definitions: &BTreeMap<String, BTreeSet<String>>,
    roots: &[&str],
    barriers: &[&str],
) -> BTreeSet<String> {
    let mut tainted = roots
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    let barriers = barriers.iter().copied().collect::<BTreeSet<_>>();
    loop {
        let additions = definitions
            .iter()
            .filter(|(name, dependencies)| {
                !barriers.contains(name.as_str())
                    && !tainted.contains(*name)
                    && intersects(dependencies, &tainted)
            })
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        if additions.is_empty() {
            return tainted;
        }
        tainted.extend(additions);
    }
}

fn collect_dependency_closure(
    name: &str,
    definitions: &BTreeMap<String, BTreeSet<String>>,
    closure: &mut BTreeSet<String>,
) {
    if !closure.insert(name.to_owned()) {
        return;
    }
    if let Some(dependencies) = definitions.get(name) {
        for dependency in dependencies {
            collect_dependency_closure(dependency, definitions, closure);
        }
    }
}

fn caller_visible(visibility: &Visibility) -> bool {
    match visibility {
        Visibility::Public(_) => true,
        Visibility::Restricted(restricted) => restricted.path.is_ident("crate"),
        Visibility::Inherited => false,
    }
}

fn intersects(left: &BTreeSet<String>, right: &BTreeSet<String>) -> bool {
    left.iter().any(|name| right.contains(name))
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

    let second_inherent_impl = r#"
impl ProcessBatchPlan {
    pub(crate) fn scope_name(&self) -> &str {
        unreachable!()
    }
}
impl ProcessBatchPlan {
    pub(crate) fn exact_identity(&self) -> &ProcessTarget {
        unreachable!()
    }
}
"#;
    assert!(caller_visible_type_leaks(
        second_inherent_impl,
        &["ProcessTarget"],
        &["ProcessBatchPlan"]
    )
    .is_err());

    let free_function_seam = r#"
pub(crate) fn inspect_plan(
    plan: &ProcessBatchPlan,
) -> &ProcessTarget {
    unreachable!()
}
"#;
    assert!(caller_visible_type_leaks(
        free_function_seam,
        &["ProcessTarget"],
        &["ProcessBatchPlan", "ProcessBatchPreview"]
    )
    .is_err());

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
    assert!(caller_visible_type_leaks(
        trait_seam,
        &["ProcessTarget"],
        &["ProcessBatchPlan", "ProcessBatchPreview"]
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

    let import_renamed_expansion_wrapper = r#"
use crate::process_control::core::ExpansionSnapshot as HiddenCatalog;
struct CatalogEnvelope {
    snapshot: HiddenCatalog,
}
pub(crate) type CallerCatalog = CatalogEnvelope;
"#;
    assert!(caller_visible_type_leaks(
        import_renamed_expansion_wrapper,
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
