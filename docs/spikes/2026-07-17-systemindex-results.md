# SystemIndex Spike Results - 2026-07-17

## Decision

**No-Go (observed host I/O violation).**

The structured `System.FileName + COP_VALUE_CONTAINS` query, explicit `SetScope`, indexed sentinel, unindexed sentinel, and service-off fail-fast behavior all worked on the reference machine. ProcMon case A nevertheless attributed five `QueryDirectory` operations and eleven non-allowlisted `ReadFile` operations to the spike PID.

One additional `Process Create` row was the Windows console host created by the harness. Excluding that row does not change the decision: the retained directory and content-read evidence violates the frozen host-I/O boundary.

This decision blocks production `/find` implementation and scheduling. The current `ISearchFolderItemFactory` route failed its evidence gate; an alternative architecture requires a separate review. No production Tauri or UI code was added.

## Evidence Identifiers

These paths are local and gitignored. Raw machine paths are not committed.

- Runner: `artifacts/systemindex-spike/20260717-220321`
- Sentinel: `spikes/systemindex/target/sentinels/manifest-861431ef1e204d29a9be87c0cc3bc416.evidence.json`
- Service-off: `spikes/systemindex/target/sentinels/failfast-20260717-220332`
- ProcMon capture: `artifacts/systemindex-spike/io-20260717-220335`
- Case A raw evidence: `case-A/A.pml`, `A-full.csv`, `A-filtered.csv`, `A-classified.csv`, and `A-forbidden.csv`
- Cases B-D: not run because the gate stopped at the first failing case.

`<USER>` below replaces the local profile directory segment. Exact values remain in the local evidence.

## Reference Environment

| Field | Observed value |
|---|---|
| Captured | `2026-07-17T22:03:36.6073792+08:00` |
| Windows | Microsoft Windows 11 Pro for Workstations, version `10.0.26100`, build `26100`, 64-bit |
| CPU | AMD Ryzen 9 7945HX with Radeon Graphics |
| Memory | `67,847,614,464` bytes |
| Storage | SAMSUNG MZVL21T0HCLR-00BL2, Fixed hard disk media, `1,024,203,640,320` bytes |
| WSearch before capture | Auto, Running, DelayedAutoStart `1` |
| Catalog | `SystemIndex`, available |
| Rust `windows` crate | `0.62.2` |
| ProcMon | x64 v4.04, valid Microsoft signature |
| ProcMon SHA-256 | `A7AB46FBE97FBC03AB66CEB786EC8C38C6588CCF21BF390F314857D1FD91D608` |
| Shell elevation | `true` |

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
| `status --json` | 0 | n/a | `0/0/0` | `1024.689 ms` | Pass |
| `scopes --json` | 0 | 2 included roots, 19 exclusions | `0/0/0` | `1027.722 ms` | Pass |
| `uipilot-spike-probe` | 0 | 0 | `1/1/1` | `1019.048 ms` | Pass |
| `uipilot-indexed-9c2c6630f4474639bc99c66491cc9c1d.txt` sentinel polling | 0 | 1 exact canonical path | `1/1/1` | `4056.132 ms` until observed | Pass |
| `uipilot-unindexed-2c24a1f03c604c34a1383df348f7f2b8.txt` | 0 | 0 | `1/1/1` | `1022.809 ms` | Pass |
| WSearch stopped | 2 | structured `notRunnable` error | `0/0/0` | not recorded | Pass |
| ProcMon case A indexed sentinel | 0 | 1 exact canonical path | `1/1/1` | `3034.197 ms` | Functional Pass; host-I/O Fail |

The service-off evidence records identical before/after state: StartMode `Auto`, State `Running`, DelayedAutoStart `1`. While stopped, the query exited `2` before Search Folder creation or enumeration. WSearch is currently Running and no ProcMon process remains.

The indexed and unindexed sentinel files, their unique directories, and live manifests were removed successfully. Only gitignored evidence remains.

Automated verification: 15 Rust tests passed; `cargo clippy --all-targets -- -D warnings` passed. Boundary tests cover quotes, wildcards, percent/underscore/brackets, backslashes, spaces, CJK, emoji, composed/decomposed Unicode, 256/257 scalars, operation order, fail-fast preconditions, and real Windows Search construction.

## Required Cases

| Acceptance statement | Status | Evidence/reason |
|---|---|---|
| Crawl Scope Manager roots and exclusions are recorded | Pass | Runner and sentinel evidence |
| Real query uses `MakeLeaf`, `SetCondition`, and explicit `SetScope` before enumeration | Pass | Real integration test and `1/1/1` counters |
| Unindexed sentinel returns zero results | Pass | Sentinel evidence, 0 results |
| Indexed sentinel is observed by exact canonical path | Pass | Sentinel and case A evidence, 1 exact result |
| Stopped WSearch fails before Search Folder creation | Pass | Exit `2`, counters `0/0/0` |
| WSearch exact state restoration is proven | Pass | Before/after snapshots match exactly |
| ProcMon case A: indexed sentinel host I/O | **Fail** | 5 `QueryDirectory`, 11 non-allowlisted `ReadFile` rows |
| ProcMon cases B-D | Not run | Gate stopped after case A failed; cannot repair case A |
| No production Tauri/UI code or fallback indexer was added | Pass | Changes are isolated to `spikes/systemindex`, scripts, and docs |
| No explicit `std::fs`, directory recursion, WSSQL, AQS, or fallback query exists in the spike | Pass (static) | Runtime Shell/Search calls still caused forbidden host I/O |

## I/O Classification

Case A retained 5,040 rows for exact spike PID `69768`. Seventeen were marked forbidden:

- 1 `Process Create`: `C:\Windows\System32\conhost.exe`. This is harness/console startup noise and is not needed for the No-Go decision.
- 5 `QueryDirectory`: `C:\ProgramData`, its `Microsoft`, `Windows`, and `Start Menu` descendants, and `C:\Users`.
- 11 `ReadFile`: `desktop.ini` under Start Menu, `C:\Users`, and the current user's Desktop, Documents, Music, Pictures, Videos, Downloads, and OneDrive; plus `C:\$MapAttributeValue` and `D:\$Extend\$UsnJrnl:$J:$DATA`.

The 16 filesystem rows remain forbidden after discounting conhost. They are attributed to the host PID, not Windows Search service processes, so the frozen requirement of no host directory enumeration/content read fails.

Raw evidence hashes:

| File | SHA-256 |
|---|---|
| `A.pml` | `A79477E70AD4288AA56C09C92899793A329AE0E013A78CD3339BB12D2F0BE3BA` |
| `A-full.csv` | `3D2C86DAE5160AADA77831CBEB529B9CDD8B2E4175EB59E9FCDBA89D90DD6A56` |
| `A-filtered.csv` | `013CCFC86B947653F50C37ABDBDFC77454A5C7FE57499F93573444A432F0AA47` |
| `A-classified.csv` | `3F0E7933B93FCE8381DED61E9B0FA9D2F6C026C5EE9F22F1ED3797C718A02F57` |
| `A-forbidden.csv` | `D98265C53F9FD0DF40D0013242BE3E8881B442FB541FD09A75BDCB69A2A429E0` |

The reproducible procedure remains in `docs/spikes/systemindex-evidence-protocol.md`. This route must not enter production planning; the next activity is a separately reviewed alternative-architecture assessment.
