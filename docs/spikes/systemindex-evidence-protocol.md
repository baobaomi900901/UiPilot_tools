# SystemIndex Host I/O Evidence Protocol

## Purpose

This protocol determines whether the standalone `systemindex-spike` process delegates file-name search to Windows Search without enumerating directories or reading target-file contents itself. It does not authorize production `/find` implementation.

## Fixed Preconditions

- Reference platform: Windows 11 x64, elevated PowerShell.
- Search backend: `SystemIndex` only, with Crawl Scope Manager `file:` roots passed through explicit `ISearchFolderItemFactory::SetScope`.
- Capture tool: Microsoft Sysinternals Process Monitor x64 v4.04 from <https://learn.microsoft.com/sysinternals/downloads/procmon>.
- `$env:PROCMON64_EXE` must resolve to Microsoft-signed `Procmon64.exe` with numeric product version major `4`, minor `4`.
- No Process Monitor instance may be running before capture.
- A missing permission, tool, valid scope, sentinel, or unambiguous trace is `Not Runnable`; it is never evidence of a pass. No WPR or other automatic fallback is permitted.

## Trace Matrix

| Case | Search state | Query target | Required functional result | Required host result |
|---|---|---|---|---|
| A | Healthy | Proven indexed sentinel | Exact canonical sentinel path appears | No host directory enumeration or target-content read |
| B | Healthy | Proven unindexed sentinel | Zero results | No host directory enumeration or target-content read |
| C | Stopped | `uipilot-index-service-off-proof` | Non-zero exit; all operation counters remain zero | No host directory enumeration or target-content read |
| D | Healthy | One literal containing quotes, `*`, `?`, `%`, `_`, brackets, CJK, emoji, and a combining character | Echoed literal is unchanged; query completes | No host directory enumeration or target-content read |

Each case starts a fresh unfiltered capture before its single spike process. Case C stops WSearch only after capture starts and restores its exact prior running/start configuration in `finally`.

## Required Evidence

One timestamped directory under `artifacts/systemindex-spike/` contains:

- `tool.json`: original product version, SHA-256, signature status and signer subject.
- `environment.json`: exact OS, CPU, memory, storage, and initial Search service state.
- Per case: raw `.pml`, full CSV, spike stdout/stderr, exit/PID/elapsed metadata, PID-filtered CSV, classified CSV, forbidden CSV, and summary JSON.
- Case C: before/after WSearch state.

Raw files can contain local paths and are gitignored. They must not be committed.

The full CSV must contain these columns. The filtered CSV selects them in this exact order:

```text
Time of Day, Process Name, PID, Operation, Path, Result, Detail
```

Rows are filtered only by the exact spike PID. The spike PID must have no `Process Create` event.

## Classification Rules

Every PID-filtered row receives a classification. File-system evidence uses these rules:

- `Load Image`, the exact spike executable, and `.dll`, `.mui`, `.nls`, or `.manifest` reads below Windows system directories: `executable/DLL load`.
- Spike stdout/stderr and evidence-directory I/O: `configuration/evidence output`.
- Registry operations: `configuration`.
- `QueryDirectory`: `directory enumeration` and forbidden.
- Any other `ReadFile`: `file content read` and forbidden until manually explained by retained evidence.
- File create/open/close and file-information queries: `metadata read`.
- Search protocol/device-control operations: `index API side effect`.
- Process/thread and network operations remain separately labelled; they cannot be silently treated as file evidence.

Windows Search service and index-database I/O is allowed only when the trace attributes it to a non-spike process. Filtering by the spike PID must not reattribute service activity to the host.

## Decision Rule

The reviewer inspects every filtered file-system row and every forbidden row for cases A-D. Any unexplained `QueryDirectory`, target-file `ReadFile`, child process, failed service restoration, missing raw artifact, or ambiguous classification is `Fail` or `Not Runnable` and makes the final Spike decision `No-Go`.

`Go` requires all four functional checks, all service restoration checks, and all four host-I/O classifications to pass with retained raw evidence.
