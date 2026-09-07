# Stage 2B Task 2 Report: Frozen Process-Tree Planning

## Scope

Implemented only the Task 2 internal process-tree planner and its Fake Adapter tests:

- `src-tauri/src/process_control/batch.rs`
- `src-tauri/src/process_control/tests/batch.rs`

No Task 3 planning facade, same-name planner, execution path, platform adapter, Cargo metadata, public API, UI, or real process side effect was added.

## TDD Evidence

### RED

Command:

```text
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::batch::batch_tree -- --nocapture
```

Expected failure observed before production implementation:

```text
error[E0432]: unresolved import `crate::process_control::batch::build_tree_plan`
no `build_tree_plan` in `process_control::batch`
error: could not compile `uipilot` (lib test) due to 1 previous error
```

This was the intended RED: the tests named the approved internal seam before it existed.

### Focused GREEN

Final command:

```text
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::batch::batch_tree -- --nocapture
```

Result:

```text
running 10 tests
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 1089 filtered out
```

Coverage includes root-only selection, root/child/grandchild depth order, stable sibling PID order, exact frozen preview values, no adapter calls after preparation, direct Stage 1 omission mapping, blocked descendants, time inversion, missing parent, zero-parent normalization, self-loop, closed cycle, root-incoming reachable cycle, ordinary root parent/sibling exclusion, duplicate PID branches, one omission per candidate, and the 256/257 boundary.

## Final Verification

Serialized process-control suite:

```text
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control -- --test-threads=1 --nocapture
```

Result:

```text
test result: ok. 122 passed; 0 failed; 1 ignored; 0 measured; 976 filtered out
```

The ignored test is the existing explicitly spawned Windows fixture child. The existing protected-target probe reported its expected environment N/A and passed.

Formatting and diff checks:

```text
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
git diff --check
```

Both exited successfully. Git emitted only the repository's line-ending conversion warning for the two Rust files; no whitespace error was reported.

## Algorithm And Omission Interpretation

The builder consumes `PreparedBatchPlanning` and never retains or calls its adapter. It indexes the immutable expansion snapshot by PID, then resolves each non-root candidate's parent chain upward:

- A unique, reliable chain reaching the exact root is selected when every reliable edge has nondecreasing creation time and the root is not re-entered through its observed incoming edge.
- A fully reliable chain that terminates at another top-level process is outside the selected tree and is not an omission. Raw parent hint `Some(0)` is normalized to the same top-level meaning as `ProcessRecord`.
- A candidate whose chain has a missing endpoint, duplicate endpoint, cycle, creation-time inversion, or unreliable ancestor is counted once as `ancestryUnverified`.
- An unreliable candidate directly below an otherwise completely reliable chain to the selected root retains its precise Stage 1 omission (`exited_or_changed`, `inspection_denied`, or `identity_unavailable`). Its descendants are `ancestryUnverified` and are never reconnected across it.
- Duplicate-PID entries take the design's explicit `ancestryUnverified` classification, one count per candidate entry, rather than reusing the catalog's generic observation omission.
- The exact root is always selected once and is never counted as an omission, including a root-incoming-edge cycle.

Selected records receive immutable depths and are sorted by depth descending, PID ascending, then creation time ascending. Exact `ProcessTarget` values are copied only into the private plan target vector; preview construction copies only the approved display fields. The reliable count is checked after selection and rejects 257 without constructing a partial plan.

## Self-Review And Concerns

The only new internal production declaration is `pub(super) fn build_tree_plan(PreparedBatchPlanning)`, and the exact source golden was updated for that seam. No same-name builder, facade method, or execution behavior was introduced.

No material unresolved concern remains for Task 2. Conservative unresolved-ancestry counting follows design section 8.2: an invalid chain that cannot prove termination outside the selected root is incomplete, while a fully reliable chain ending elsewhere is provably out of scope.
