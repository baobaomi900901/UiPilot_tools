# Stage 2C Task 1: Restart Path Feasibility

Status: **BLOCKED — path-to-child handshake property not demonstrated**

Date: 2026-09-08
Branch: `codex/plugin-platform-roadmap`
Implementation-start base: `3e027b3`
Environment: Windows standard user; owned files, directories, handles, and process only.

## Experiment

The fixture copied the current Rust test executable to a test-owned path containing
spaces and Unicode, and created separate owned working and control directories. It
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

## Demonstrated Before The Blocker

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
- Partial acquisition failure and intentional unwind were exercised, but the
  subsequent rename targeted a different probe directory, not an acquired lease
  component. This does not prove lease release; that verification remains open.
- `CreateProcessW` returned success while every executable and working-directory
  lease remained held.

## Blocker Evidence

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
path then requested termination, waited boundedly, and closed both returned
handles. The temporary-tree guard made a best-effort recursive removal during
unwind; because the test stopped at the handshake assertion and cleanup was not
separately inspected afterward, complete tree removal is not claimed.

Because the required owned child did not publish its actual arguments, effective
working directory, and explicit environment marker, this experiment does **not**
prove that the intended image and CWD were successfully reached. Execution also
did not reach the normal post-child assertions that the exact executable and
ancestor mutations succeed after releasing the successful launch leases. Those
facts are therefore not claimed.

The successful-launch lease vectors remained alive through the handshake wait,
although the design requires them only through `CreateProcessW` return. The
explicit environment block was supplied to creation and included the marker and
control path, but child receipt of it was not demonstrated. These are possible
fixture/harness factors; this result is BLOCKED/INCONCLUSIVE, not evidence that
the approved path policy is fundamentally impossible.

The affected process-control regression, formatting and Clippy gates have not been
run at this experimental revision, and no success is claimed for them.
No production restart module or Task 2 work was started.

## Independent Correctness And Scope Review

Independent review on 2026-09-08 confirmed the four-file scope and the
BLOCKED/INCONCLUSIVE gate, and identified the unsupported lease-release claim
corrected above. The earlier text under this heading was author assessment, not
an independent approval. Task 1 is not accepted and Task 2 must not begin.

The narrow next diagnostic is to release successful-launch leases after
CreateProcessW returns and before waiting for the child handshake, matching the
approved design rather than holding them until child exit. This is a hypothesis,
not a demonstrated root cause; no further experiment was run after the repeated
blocker stop. A subsequent release regression must target actually leased
components, not an unrelated probe directory.

A controller read-only post-run scan found zero `UiPilot restart feasibility *`
directories under the current temporary directory. This establishes directory
residue absence at that scan, not the still-unproven per-path cleanup assertions.
