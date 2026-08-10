# Normal-User Dev `/find` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `npm run tauri dev` use a protected, already-installed Everything Service while UiPilot and the Everything query client remain at normal user integrity, and reject an empty/unready index instead of reporting a valid no-match result.

**Architecture:** A focused PowerShell helper validates the default Everything Service and selects only an Everything client in the current interactive session. The Rust adapter performs a one-time empty-query capability probe before caching a newly connected Query2 client, so subsequent legitimate zero-match searches remain valid.

**Tech Stack:** PowerShell 5.1, Rust 1.96, Everything 1.4.1.1032 Query2 IPC, Tauri 2.

## Global Constraints

- Scope is dev only; do not add NSIS, production service ownership, upgrades, VM gates, or a folder-index fallback.
- `npm run tauri dev`, UiPilot, Vite, Cargo, and the Everything user client must never request elevation.
- The user performs the one-time official Everything Service installation and any UAC interaction manually.
- Never automate mouse or keyboard input and never launch a focus-stealing GUI as part of tests.
- Never install a LocalSystem service whose binary path is inside the repository, a temp directory, or another normal-user-writable directory.
- Reuse the default Everything instance only for this dev slice; do not probe, stop, or mutate unrelated processes by name alone.

---

### Task 1: Testable Everything Dev Runtime Boundaries

**Files:**
- Create: `scripts/dev-everything-runtime.ps1`
- Create: `scripts/test-dev-with-everything.ps1`
- Modify: `scripts/dev-with-everything.ps1:1-52`

**Interfaces:**
- Consumes: `Win32_Service` snapshots with `Name`, `State`, and `ProcessId`; `Win32_Process` snapshots with `ProcessId` and `ExecutablePath`; `System.Diagnostics.Process` objects with `ProcessName`, `SessionId`, and `Id`.
- Produces: `Test-ProtectedEverythingServicePath -ExecutablePath <string> -ProgramFilesRoots <string[]> -> bool`; `Select-EverythingUserClient -Processes <object[]> -SessionId <int> -> object|null`; `Get-EverythingServiceFailure -Service <object|null> -ServiceProcess <object|null> -ProgramFilesRoots <string[]> -> string|null`.

- [ ] **Step 1: Write the failing PowerShell boundary tests**

Create `scripts/test-dev-with-everything.ps1` with strict mode, dot-source `dev-everything-runtime.ps1`, and assert these exact cases:

```powershell
$processes = @(
    [pscustomobject]@{ ProcessName = 'Everything'; SessionId = 0; Id = 10 },
    [pscustomobject]@{ ProcessName = 'Everything'; SessionId = 4; Id = 20 },
    [pscustomobject]@{ ProcessName = 'Other'; SessionId = 4; Id = 30 }
)
$selected = Select-EverythingUserClient -Processes $processes -SessionId 4
Assert-Condition ($selected.Id -eq 20) 'Session 0 service must not replace the interactive client'
Assert-Condition ($null -eq (Select-EverythingUserClient -Processes $processes -SessionId 7)) 'Missing interactive client must return null'

$roots = @('C:\Program Files', 'C:\Program Files (x86)')
Assert-Condition (Test-ProtectedEverythingServicePath 'C:\Program Files\Everything\Everything.exe' $roots) 'Program Files service should pass'
Assert-Condition (-not (Test-ProtectedEverythingServicePath 'C:\Program Files-Evil\Everything.exe' $roots)) 'Prefix lookalike must fail'
Assert-Condition (-not (Test-ProtectedEverythingServicePath 'D:\code\UiPilot_tools\src-tauri\resources\everything\Everything.exe' $roots)) 'Repository service must fail'

$running = [pscustomobject]@{ Name = 'Everything'; State = 'Running'; ProcessId = 10 }
$serviceProcess = [pscustomobject]@{ ProcessId = 10; ExecutablePath = 'C:\Program Files\Everything\Everything.exe' }
Assert-Condition ($null -eq (Get-EverythingServiceFailure $running $serviceProcess $roots)) 'Protected running service should pass'
Assert-Condition ((Get-EverythingServiceFailure $null $null $roots) -ceq 'EVERYTHING_SERVICE_MISSING') 'Missing service must be stable'
Assert-Condition ((Get-EverythingServiceFailure ([pscustomobject]@{ Name='Everything'; State='Stopped'; ProcessId=0 }) $null $roots) -ceq 'EVERYTHING_SERVICE_NOT_RUNNING') 'Stopped service must be stable'
```

Add a source boundary assertion that `dev-with-everything.ps1` dot-sources the helper and does not use `Get-Process -Name 'Everything' | Select-Object -First 1`.

- [ ] **Step 2: Run the test to verify it fails**

Run:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts\test-dev-with-everything.ps1
```

Expected: FAIL because `scripts/dev-everything-runtime.ps1` does not exist.

- [ ] **Step 3: Implement the pure helper functions**

Create `scripts/dev-everything-runtime.ps1` with no top-level process or service mutations:

```powershell
Set-StrictMode -Version Latest

function Test-ProtectedEverythingServicePath {
    param([string]$ExecutablePath, [string[]]$ProgramFilesRoots)
    if ([string]::IsNullOrWhiteSpace($ExecutablePath)) { return $false }
    $candidate = [IO.Path]::GetFullPath($ExecutablePath)
    foreach ($root in $ProgramFilesRoots) {
        if ([string]::IsNullOrWhiteSpace($root)) { continue }
        $boundary = [IO.Path]::GetFullPath($root).TrimEnd('\') + '\'
        if ($candidate.StartsWith($boundary, [StringComparison]::OrdinalIgnoreCase)) { return $true }
    }
    return $false
}

function Select-EverythingUserClient {
    param([object[]]$Processes, [int]$SessionId)
    $matches = @($Processes | Where-Object {
        $_.ProcessName -ceq 'Everything' -and $_.SessionId -eq $SessionId
    } | Sort-Object Id | Select-Object -First 1)[0]
    if ($matches.Count -eq 0) { return $null }
    return $matches[0]
}

function Get-EverythingServiceFailure {
    param([object]$Service, [object]$ServiceProcess, [string[]]$ProgramFilesRoots)
    if ($null -eq $Service) { return 'EVERYTHING_SERVICE_MISSING' }
    if ([string]$Service.State -cne 'Running' -or [uint32]$Service.ProcessId -eq 0) { return 'EVERYTHING_SERVICE_NOT_RUNNING' }
    if ($null -eq $ServiceProcess -or [uint32]$ServiceProcess.ProcessId -ne [uint32]$Service.ProcessId) { return 'EVERYTHING_SERVICE_PROCESS_UNAVAILABLE' }
    if (-not (Test-ProtectedEverythingServicePath ([string]$ServiceProcess.ExecutablePath) $ProgramFilesRoots)) { return 'EVERYTHING_SERVICE_PATH_UNSAFE' }
    return $null
}
```

- [ ] **Step 4: Run the boundary tests**

Run the command from Step 2.

Expected: helper tests PASS, followed by a source-boundary failure because the main dev script has not been migrated.

- [ ] **Step 5: Migrate the dev startup script**

Modify `scripts/dev-with-everything.ps1` to:

1. Dot-source `dev-everything-runtime.ps1`.
2. Read the `Everything` service with `Get-CimInstance Win32_Service -Filter "Name='Everything'"`.
3. Resolve its live executable through `Win32_Process` using the service `ProcessId`, not by parsing a command line.
4. Validate against non-empty `$env:ProgramFiles` and `${env:ProgramFiles(x86)}` roots.
5. Select only an Everything process whose `SessionId` equals `(Get-Process -Id $PID).SessionId`.
6. Start the reviewed resource client at normal integrity only when that interactive process is absent.
7. Poll `FindWindowW('EVERYTHING_TASKBAR_NOTIFICATION', NULL)` for at most 10 seconds instead of sleeping for a fixed 10 seconds.
8. In `finally`, request `Everything.exe -quit`, wait up to 2 seconds for the exact owned PID, and use `Stop-Process -Id $ownedProcess.Id` only as the final fallback.

Map the stable preflight errors to actionable terminal messages. For `EVERYTHING_SERVICE_MISSING`, print:

```text
Everything Service is required for normal-user dev. Install the official Everything Service once, then rerun npm run tauri dev.
```

Do not add `-Verb RunAs`, `Start-Process powershell`, UI automation, or service installation to this script.

- [ ] **Step 6: Run the PowerShell test and package verifier**

Run:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts\test-dev-with-everything.ps1
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts\test-everything-package.ps1
```

Expected: `DEV_EVERYTHING_RUNTIME_PASS` and `EVERYTHING_PACKAGE_PASS`.

- [ ] **Step 7: Commit Task 1**

```powershell
git add scripts/dev-everything-runtime.ps1 scripts/test-dev-with-everything.ps1 scripts/dev-with-everything.ps1
git commit -m "fix: require safe Everything service for dev"
```

### Task 2: Reject an Unready Everything Index Before Caching

**Files:**
- Modify: `src-tauri/src/file_search/everything.rs:37-57`
- Modify: `src-tauri/src/file_search/everything.rs:211-267`
- Test: `src-tauri/src/file_search/everything.rs:280-720`

**Interfaces:**
- Consumes: `EverythingClient::connect(&str, Duration)`, `EverythingClient::query(EverythingQuerySpec)`.
- Produces: `connect_ready_with<C, Connect, Probe>(connect: Connect, probe: Probe) -> Result<C, EverythingClientError>`; a client is returned only when an empty Query2 probe reports `total > 0`.

- [ ] **Step 1: Write failing Rust readiness tests**

Import `connect_ready_with` into the existing test module and add:

```rust
#[test]
fn connection_requires_a_nonempty_loaded_index() {
    let captured = RefCell::new(None);
    let empty = connect_ready_with(
        || Ok(TestClient { id: 1 }),
        |_, spec| {
            *captured.borrow_mut() = Some(spec);
            Ok(EverythingQueryResult {
                total: 0,
                request_flags: 0x155,
                sort_type: 14,
                items: Vec::new(),
            })
        },
    );
    assert!(matches!(empty, Err(EverythingClientError::IpcUnavailable)));
    let spec = captured.borrow();
    let spec = spec.as_ref().unwrap();
    assert!(spec.search.is_empty());
    assert_eq!(spec.max_results, 1);

    assert_eq!(
        connect_ready_with(
            || Ok(TestClient { id: 2 }),
            |_, _| Ok(EverythingQueryResult {
                total: 1,
                request_flags: 0x155,
                sort_type: 14,
                items: Vec::new(),
            }),
        )
        .unwrap()
        .id,
        2
    );
}
```

Add a cache test proving a failed readiness probe leaves `slot` as `None`; retain the existing test that a legitimate user query returning zero items is successful after a client is cached.

- [ ] **Step 2: Run the focused test to verify it fails**

Run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml file_search::everything::tests::connection_requires_a_nonempty_loaded_index
```

Expected: FAIL because `connect_ready_with` does not exist.

- [ ] **Step 3: Implement the readiness connection helper**

Add a probe spec with `search: Vec::new()`, `max_results: 1`, request flags `0x155`, date-modified descending sort, and a one-second checked deadline. Implement:

```rust
fn connect_ready_with<C, Connect, Probe>(
    connect: Connect,
    probe: Probe,
) -> Result<C, EverythingClientError>
where
    Connect: FnOnce() -> Result<C, EverythingClientError>,
    Probe: FnOnce(&C, EverythingQuerySpec) -> Result<EverythingQueryResult, EverythingClientError>,
{
    let client = connect()?;
    let result = probe(&client, everything_index_probe_spec()? )?;
    if result.total == 0 {
        Err(EverythingClientError::IpcUnavailable)
    } else {
        Ok(client)
    }
}
```

Make `everything_index_probe_spec` return `Result<EverythingQuerySpec, EverythingClientError>` and map deadline overflow to `IpcUnavailable`. In `EverythingSearchState::search`, replace the direct connect closure with `connect_ready_with(|| EverythingClient::connect("", Duration::from_millis(250)), EverythingClient::query)`.

- [ ] **Step 4: Run focused and module tests**

Run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml file_search::everything
```

Expected: all Everything adapter tests PASS, including zero-index rejection and valid zero-match behavior.

- [ ] **Step 5: Format and check Rust**

Run:

```powershell
cargo fmt --manifest-path src-tauri\Cargo.toml -- --check
cargo check --manifest-path src-tauri\Cargo.toml --no-default-features
```

Expected: both commands exit 0.

- [ ] **Step 6: Commit Task 2**

```powershell
git add src-tauri/src/file_search/everything.rs
git commit -m "fix: reject unready Everything index"
```

### Task 3: Normal-User Local Verification

**Files:**
- Modify only if evidence requires it: `scripts/dev-with-everything.ps1`, `scripts/dev-everything-runtime.ps1`, or `src-tauri/src/file_search/everything.rs`

**Interfaces:**
- Consumes: installed official default Everything Service and `spikes/everything-ipc/target/debug/everything-ipc-spike.exe`.
- Produces: evidence that both the empty index and `windows` queries succeed from the normal user token.

- [ ] **Step 1: Run all non-GUI automated checks**

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts\test-dev-with-everything.ps1
cargo test --manifest-path src-tauri\Cargo.toml file_search::everything
npm.cmd test -- --exclude ".worktrees/**"
```

Expected: all commands exit 0. Record any pre-existing unrelated failure separately; do not modify unrelated files.

- [ ] **Step 2: Stop at the manual Service checkpoint when required**

Inspect the service without changing system state:

```powershell
Get-CimInstance Win32_Service -Filter "Name='Everything'" | Select-Object Name,State,ProcessId,PathName
```

If absent, notify the user to install Everything into `C:\Program Files\Everything`, enable `Tools -> Options -> General -> Everything Service`, leave `Run as administrator` unchecked, approve the single UAC prompt, and report completion. Do not open the UI or provide mouse/keyboard input.

- [ ] **Step 3: Verify the service and IPC from the normal token**

After the user reports completion, run:

```powershell
whoami /groups
cargo run --manifest-path spikes\everything-ipc\Cargo.toml -- --query "" --limit 1 --timeout-ms 3000 --format json
cargo run --manifest-path spikes\everything-ipc\Cargo.toml -- --query windows --limit 5 --timeout-ms 3000 --format json
```

Expected: `whoami` contains `Medium Mandatory Level`; the empty query reports `total > 0`; `windows` reports at least one returned item.

- [ ] **Step 4: Start dev without GUI automation**

Run `npm run tauri dev` only after the service and IPC probes pass. Observe terminal output for startup errors. Do not interact with the UiPilot window, tray, mouse, or keyboard.

- [ ] **Step 5: Ask the user for the sole UI acceptance check**

Tell the user to manually open UiPilot and enter `/find windows`. Acceptance requires at least one visible result and no UAC prompt during dev startup.

- [ ] **Step 6: Final regression check and commit only evidence-driven corrections**

Re-run Steps 1 and 3 after any correction. Commit only files from Tasks 1-2; leave all unrelated worktree changes untouched.
