# SystemIndex Spike Results - 2026-07-17

## Decision

**No-Go (required evidence Not Runnable).**

The structured `System.FileName + COP_VALUE_CONTAINS` query, explicit `SetScope`, indexed sentinel, and unindexed sentinel worked on the reference machine. The session was not elevated and `$env:PROCMON64_EXE` was unset, so the required service-off integration case and ProcMon A-D host-I/O matrix could not run. Missing evidence cannot be treated as a pass.

This decision blocks production `/find` implementation and scheduling. The Spike may be rerun in a separately reviewed, compliant environment; an alternative architecture requires its own review. No production Tauri or UI code was added.

## Evidence Identifiers

These paths are local and gitignored. Raw machine paths are not committed.

- Runner: `artifacts/systemindex-spike/20260717-212021`
- Sentinel: `spikes/systemindex/target/sentinels/manifest-819b815e731c4529b88a71c7352621a8.evidence.json`
- ProcMon cases A-D: no artifacts; capture exited `2` before ProcMon startup.
- Service-off: no mutation artifact; fail-fast exited `2` at the elevation gate and WSearch remained Running.

`<USER>` below replaces the local profile directory segment. Exact values remain in the local evidence.

## Reference Environment

| Field | Observed value |
|---|---|
| Captured | `2026-07-17T21:20:22.5503018+08:00` |
| Windows | Microsoft Windows 11 Pro for Workstations, version `10.0.26100`, build `26100`, 64-bit |
| CPU | AMD Ryzen 9 7945HX with Radeon Graphics |
| Memory | `67,847,614,464` bytes |
| Storage | SAMSUNG MZVL21T0HCLR-00BL2, Fixed hard disk media, `1,024,203,640,320` bytes |
| WSearch | Running, Automatic |
| Catalog | `SystemIndex`, available |
| Rust `windows` crate | `0.62.2` |
| Shell elevation | `false` |
| `PROCMON64_EXE` | unset |

## Scope Evidence

Included `file:` roots:

```text
file:///C:\ProgramData\Microsoft\Windows\Start Menu\
file:///C:\Users\
```

Observed exclusion rules, deidentified only at the profile segment:

```text
file:///*\$RECYCLE.BIN\
file:///*\DfsrPrivate\
file:///*\System Volume Information\
file:///C:\Users\*\AppData\
file:///C:\Users\*\AppData\Local\Microsoft\Windows\Temporary Internet Files\
file:///C:\Users\*\AppData\Local\Temp\
file:///C:\Users\Default\AppData\
file:///C:\Users\<USER>\.*\
file:///C:\Users\<USER>\AppData\
file:///C:\Users\<USER>\Desktop\MyShell - 副本\
file:///C:\Users\<USER>\Documents\myshell\
file:///C:\Users\<USER>\Documents\WindowsPowerShell\MyShell - 副本\
file:///C:\Users\<USER>\Documents\WindowsPowerShell\MyShell\
file:///C:\Users\<USER>\everything-claude-code\
file:///C:\Users\<USER>\MicrosoftEdgeBackups\
file:///C:\Users\<USER>\vaults\my-wiki\
file:///C:\Users\Public\Documents\Embarcadero\Studio\20.0\Samples\
file:///C:\Windows.*\
file:///C:\WINDOWS\*\temp\
```

The `status` and `scopes` commands both exited `0`; all three Search Folder counters were `0`.

## Functional Observations

| Fixture | Exit | Results | Counters: factory/scope/enumerate | Elapsed | Outcome |
|---|---:|---:|---|---:|---|
| `status --json` | 0 | n/a | `0/0/0` | `1039.437 ms` | Pass |
| `scopes --json` | 0 | 2 included roots, 19 exclusions | `0/0/0` | `1024.908 ms` | Pass |
| `uipilot-spike-probe` | 0 | 0 | `1/1/1` | `1029.4 ms` | Pass |
| `uipilot-indexed-bcedef931f0944b1b94a3ac871152e74.txt` | 0 | 1 exact canonical path | `1/1/1` | `2054.1 ms` until observed | Pass |
| `uipilot-unindexed-3e5cdeb5156e4653988fb0919560a055.txt` | 0 | 0 | `1/1/1` | `2049.424 ms` | Pass |
| Quotes + `*?%_[]` + CJK + emoji + combining character | 0 | not material | `1/1/1` | not recorded outside ProcMon case D | Literal echo Pass; I/O Not Runnable |

The indexed and unindexed sentinel files, their unique directories, and the manifest were removed successfully. Only the gitignored evidence JSON remains.

Automated verification: 15 Rust tests passed; `cargo clippy --all-targets -- -D warnings` passed. Boundary tests cover quotes, wildcards, percent/underscore/brackets, backslashes, spaces, CJK, emoji, composed/decomposed Unicode, 256/257 scalars, operation order, fail-fast preconditions, and real Windows Search construction.

## Required Cases

| Acceptance statement | Status | Evidence/reason |
|---|---|---|
| Crawl Scope Manager roots and exclusions are recorded | Pass | Runner and sentinel evidence |
| Real query uses `MakeLeaf`, `SetCondition`, and explicit `SetScope` before enumeration | Pass | Real integration test and `1/1/1` counters |
| Unindexed sentinel returns zero results | Pass | Sentinel evidence, 0 results |
| Indexed sentinel is observed by exact canonical path | Pass | Sentinel evidence, 1 exact result |
| Stopped WSearch fails before Search Folder creation | Not Runnable | Shell was not elevated; script exited `2` before service access |
| WSearch exact state restoration is proven | Not Runnable | Service was never mutated, so no stop/restore trace exists |
| ProcMon case A: indexed sentinel host I/O | Not Runnable | No elevated shell; `PROCMON64_EXE` unset |
| ProcMon case B: unindexed sentinel host I/O | Not Runnable | No elevated shell; `PROCMON64_EXE` unset |
| ProcMon case C: stopped service host I/O | Not Runnable | No elevated shell; `PROCMON64_EXE` unset |
| ProcMon case D: literal fixture host I/O | Not Runnable | No elevated shell; `PROCMON64_EXE` unset |
| No production Tauri/UI code or fallback indexer was added | Pass | Changes are isolated to `spikes/systemindex`, scripts, and docs |
| No `std::fs`, directory recursion, WSSQL, AQS, or fallback query exists in the spike | Pass (static) | Source scan and review; runtime host-I/O proof remains Not Runnable |

## I/O Classification

No `.pml`, full CSV, or PID-filtered CSV exists for cases A-D. Therefore no host file operation can be classified from runtime evidence. The absence of traces is `Not Runnable`, not proof that directory enumeration or content reads did not occur.

The reproducible procedure is defined in `docs/spikes/systemindex-evidence-protocol.md`. A future execution must use Microsoft-signed x64 ProcMon 4.04, retain all four raw traces, inspect every filtered filesystem row, and produce a new reviewed decision.
