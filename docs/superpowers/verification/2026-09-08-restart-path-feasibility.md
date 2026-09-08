# Stage 2C Task 1: Restart Path Feasibility

Status: **SUPPORTED — focused feasibility and affected regression pass; targeted review pending**

Date: 2026-09-08
Branch: `codex/plugin-platform-roadmap`
Implementation-start base: `3e027b3`
Controlled-helper implementation base: `93ff3b9`
Environment: Windows standard user; owned files, directories, handles, and process only.

## Experiment

The final fixture compiles a minimal std-only Rust helper into a test-owned path
containing spaces and Unicode, and creates separate owned working and control
directories. The compiler child is guarded and bounded; its source and output
remain inside the owned temporary tree. The fixture
resolved the executable and working directory from their DOS paths to
`\\?\Volume{GUID}\...` paths using `GetVolumePathNameW` and
`GetVolumeNameForVolumeMountPointW`.

Each relative component from the volume root was opened with
`FILE_FLAG_OPEN_REPARSE_POINT` and `FILE_SHARE_READ` only. Directories additionally
used `FILE_FLAG_BACKUP_SEMANTICS` and requested
`FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES`; the final executable requested
`GENERIC_READ`. Each acquired handle was checked for expected directory/file kind,
absence of `FILE_ATTRIBUTE_REPARSE_POINT`, and exact
`GetFinalPathNameByHandleW(VOLUME_NAME_GUID)` resolution. The fixture retained all
leases through `CreateProcessW`.

The proposed creation call used a separate non-null `lpApplicationName`, mutable
quoted command line, explicit double-NUL Unicode environment block, explicit
volume-GUID current directory, `CREATE_UNICODE_ENVIRONMENT`, and
`bInheritHandles = FALSE`. The command line carried owned arguments containing a
space, a quote, and Unicode. A test-owned control path and explicit marker were
provided for a bounded child report of arguments, effective CWD, and environment.

`PROCESS_INFORMATION` was wrapped immediately after successful creation in a
single RAII owner for both returned handles. Its process handle was independently
capable of bounded termination/wait cleanup. `WAIT_FAILED` was classified
separately from timeout and signaled completion.

## Demonstrated Properties

- Local fixed-volume resolution produced the required volume-GUID form and a
  non-empty filesystem name.
- An owned directory symlink was opened as a reparse point and rejected by the
  actual component walk before process creation.
- Unsupported UNC and alternate-data-stream path forms were rejected before the
  creation call.
- While all leases were held, write-capable and delete-capable opens were denied
  for the final executable, executable parent, and working directory. Directory
  probes used backup semantics, so their rejection was a sharing result rather
  than an invalid directory-open shape.
- While leased, final-executable rename, executable-ancestor rename, and working-
  directory rename were denied.
- An injected failure immediately before opening the final executable occurred
  after the owned executable parent had been leased. While held, that exact owned
  directory rejected write/delete opens and rename; after partial leases dropped,
  the same rename succeeded and was reversed.
- Intentional unwind held the exact owned working-directory path leases. That CWD
  rejected write/delete opens and rename while held; after unwind released the
  leases, the same rename succeeded and was reversed.
- `CreateProcessW` returned success while every executable and working-directory
  lease remained held.
- The minimal helper wrote its handshake directly from `main`, demonstrating the
  exact five argument values (`plain`, a space-containing value, a quoted value,
  Unicode, and the owned control argument), the explicit Unicode environment
  marker, and agreement between the CONTROL environment value and control arg.
- The helper's effective current directory was opened and its final volume-GUID
  path matched the leased CWD identity. Display spelling differences are not
  accepted without this handle-based identity match.
- `QueryFullProcessImageNameW` on the returned process handle matched the owned
  helper executable. The controller observed child completion through that same
  independently retained process handle.
- Successful path leases were released immediately after `CreateProcessW`
  returned. The final executable and executable-parent renames then succeeded
  and were reversed.
- Waiting on a null handle produced `WAIT_FAILED`, distinct from timeout and
  signaled completion.
- Explicit cleanup removed the complete owned temporary tree and asserted zero
  residue.

## Historical Libtest-Fixture Evidence

Compile-only command:

```text
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::restart::windows::restart_path_feasibility --no-run
```

Result: PASS, exit 0, 27.33 s.

Required focused command:

```text
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::restart::windows::restart_path_feasibility -- --nocapture --test-threads=1
```

The same bounded handshake failure occurred on three runs of that exact command:

1. The original control-file handshake timed out: exit 1, 1 failed, 1 ignored,
   1130 filtered out, 6.27 s.
2. An early-exit classifier was added without changing creation or path policy;
   the handshake still timed out, and the process was not signaled: exit 1,
   1 failed, 1 ignored, 1130 filtered out, 6.29 s.
3. The same owned control path was additionally carried in an actual quoted child
   argument to distinguish environment lookup from argument delivery; the
   handshake still timed out with the process nonsignaled: exit 1, 1 failed,
   1 ignored, 1130 filtered out, 6.65 s.

On all runs `CreateProcessW` returned a process and thread handle. The RAII cleanup
path then requested termination, attempted a bounded wait, and closed both returned
handles; the ignored wait result does not establish successful termination. The
temporary-tree guard made a best-effort recursive removal during
unwind; because the test stopped at the handshake assertion and cleanup was not
separately inspected afterward, complete tree removal is not claimed.

User-authorized narrow continuation added two more runs without changing the
path policy, creation inputs, or five-second handshake budget:

1. Successful executable/CWD leases were released immediately after
   `CreateProcessW` returned and before the handshake wait. The same timeout
   occurred: exit 1, 1 failed, 1 ignored, 1130 filtered out, 6.26 s. Changing
   successful-lease lifetime alone did not resolve the failure.
2. A read-only `QueryFullProcessImageNameW` check on the returned process handle
   matched the owned copied executable, then the same timeout occurred: exit 1,
   1 failed, 1 ignored, 1130 filtered out, 6.22 s. The returned process object
   identifies the intended executable; this does not prove that its primary
   thread or test entry executed or received the intended child context.

Those runs establish only that the copied libtest fixture did not publish its
handshake. The final controlled contrast does not prove why libtest stalled; it
removes libtest parsing from the feasibility proof.

## Controlled Minimal-Helper Evidence

The first two helper-focused invocations stopped before `CreateProcessW`: guarded
`rustc` returned exit 1, then inherited stderr identified the concrete setup error
`invalid character ' ' in crate name: startup helper`. Adding only
`--crate-name uipilot_restart_feasibility_helper` corrected that compiler
invocation. These were compiler setup failures, not path-feasibility attempts.

After that correction, the controlled helper reached its handshake and all path,
argument, environment, CWD-identity, release, wait-classification, child-completion,
and zero-residue assertions. One intermediate focused run reached all substantive
proofs but failed the final WAIT_FAILED fixture because `HANDLE(-1)` is a valid
current-process pseudo-handle and returned `WAIT_TIMEOUT`. Replacing it with the
documented invalid null handle corrected only that test datum.

Final focused command: PASS, exit 0, 1 passed, 0 failed, 0 ignored, 1130 filtered
out, test 0.39 s (compile 28.41 s). Reported filesystem was NTFS and effective
volume root was `\\?\Volume{86bc9793-23b4-468b-b1a6-19b8d91ad2d2}\`.

Affected serialized command:

```text
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control -- --nocapture --test-threads=1
```

Result: PASS, exit 0, 153 passed, 0 failed, 2 ignored, 976 filtered out,
22.96 s. This is the prior 152-pass baseline plus the new feasibility test.

`cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: PASS after applying
rustfmt to the new test module.

Default Clippy command failed with the accepted inherited aggregate only:
12 library errors and 13 library-test errors. No diagnostic referenced
`process_control/tests/restart/windows.rs`; therefore the new module contributes
zero Clippy diagnostics. No inherited warning was suppressed or changed.

No production restart module or Task 2 work was started.

## Independent Correctness And Scope Review

An earlier targeted review accepted only the historical BLOCKED/INCONCLUSIVE
record and requested the corrected owned-component release probes. The final
controlled-helper PASS and scope are awaiting a new targeted read-only review;
this record does not self-approve Task 1 or authorize Task 2.

Earlier controller scanning found no historical fixture residue. The final passing
test additionally performs and asserts removal of its own complete temporary tree.
