# Everything CLI Tracer Bullet Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the existing Everything Query2 spike into a command-line tool that prints real file and directory results from a running Everything 1.4 instance.

**Architecture:** Keep argument parsing and output rendering in a dependency-light `cli` module, while `main.rs` remains a thin adapter over the existing `EverythingClient`. The CLI sends exactly one bounded Query2 request and renders the existing `EverythingQueryResult`; no second search implementation or process wrapper is introduced.

**Tech Stack:** Rust 2021, existing `windows` crate Query2 client, `serde`, `serde_json`, Cargo tests, Windows Everything 1.4.

**Source Spec:** `docs/superpowers/specs/2026-07-29-everything-cli-tracer-bullet-design.md`

## Global Constraints

- Modify only `spikes/everything-ipc`; do not touch UiPilot production code, frontend code, installer hooks, resources, Service lifecycle, UAC, ACL, or Owner logic.
- Use the existing `EverythingClient`, `EverythingQuerySpec`, `EverythingQueryResult`, and Query2 reply window.
- Do not call `ES.exe`, PowerShell search commands, filesystem traversal APIs, or spawn Everything from the CLI.
- The default query limit is `20`; accepted limits are `1..=200`.
- The default timeout is `1000` ms; accepted timeouts are `1..=60000` ms.
- The default instance is the empty string, which maps to `EVERYTHING_TASKBAR_NOTIFICATION` in the existing client.
- The default output is text; `--format json` emits one valid JSON object.
- Sort remains fixed to date-modified descending for this tracer bullet.
- Every non-trivial behavior starts with a failing test. Each task is reviewed before the next task begins.

## Cross-Task Interfaces

`spikes/everything-ipc/src/cli.rs` owns these interfaces:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliArgs {
    pub instance: String,
    pub query: String,
    pub limit: u32,
    pub timeout: Duration,
    pub format: OutputFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliError {
    InvalidArguments,
    InvalidLimit,
    InvalidTimeout,
    InvalidFormat,
    RenderFailed,
}

pub fn parse_args(
    args: impl IntoIterator<Item = OsString>,
) -> Result<CliArgs, CliError>;

pub fn render_result(
    format: OutputFormat,
    result: &EverythingQueryResult,
) -> Result<String, CliError>;
```

The binary prints stable error codes:

```text
E_ARGUMENTS
E_LIMIT
E_TIMEOUT_ARGUMENT
E_FORMAT
E_EVERYTHING_UNAVAILABLE
E_QUERY_TIMEOUT
E_PROTOCOL
E_IPC
E_RENDER
```

Exit code `2` is a CLI contract error, `3` is unavailable/timeout, and `4` is protocol/IPC/render failure.

---

### Task 1: Add the Pure CLI Contract and Renderers

**Files:**
- Modify: `spikes/everything-ipc/Cargo.toml`
- Modify: `spikes/everything-ipc/src/lib.rs`
- Create: `spikes/everything-ipc/src/cli.rs`

**Interfaces:**
- Consumes: `EverythingQueryResult` and `EverythingResultItem` from `protocol.rs`.
- Produces: `CliArgs`, `CliError`, `OutputFormat`, `parse_args`, and `render_result` from `Cross-Task Interfaces`.

- [ ] **Step 1: Add failing parser and renderer tests**

Create `src/cli.rs` with the public types and function signatures above. Add unit tests covering the exact defaults and invalid boundaries:

Start with these deliberately incomplete bodies so the tests fail for a known reason:

```rust
pub fn parse_args(
    _args: impl IntoIterator<Item = OsString>,
) -> Result<CliArgs, CliError> {
    Err(CliError::InvalidArguments)
}

pub fn render_result(
    _format: OutputFormat,
    _result: &EverythingQueryResult,
) -> Result<String, CliError> {
    Err(CliError::RenderFailed)
}
```

```rust
#[test]
fn parses_defaults() {
    let args = parse_args(["--query", "*.rs"].map(OsString::from)).unwrap();
    assert_eq!(args.instance, "");
    assert_eq!(args.query, "*.rs");
    assert_eq!(args.limit, 20);
    assert_eq!(args.timeout, Duration::from_millis(1_000));
    assert_eq!(args.format, OutputFormat::Text);
}

#[test]
fn parses_explicit_values_in_any_order() {
    let args = parse_args([
        "--format", "json", "--limit", "200", "--query", "report",
        "--timeout-ms", "60000", "--instance", "Work",
    ].map(OsString::from)).unwrap();
    assert_eq!(args.instance, "Work");
    assert_eq!(args.limit, 200);
    assert_eq!(args.timeout, Duration::from_millis(60_000));
    assert_eq!(args.format, OutputFormat::Json);
}

#[test]
fn rejects_invalid_contract_values() {
    assert_eq!(parse_args(["--limit", "0", "--query", "x"].map(OsString::from)), Err(CliError::InvalidLimit));
    assert_eq!(parse_args(["--limit", "201", "--query", "x"].map(OsString::from)), Err(CliError::InvalidLimit));
    assert_eq!(parse_args(["--timeout-ms", "0", "--query", "x"].map(OsString::from)), Err(CliError::InvalidTimeout));
    assert_eq!(parse_args(["--format", "xml", "--query", "x"].map(OsString::from)), Err(CliError::InvalidFormat));
    assert_eq!(parse_args(["--query", "x", "--query", "y"].map(OsString::from)), Err(CliError::InvalidArguments));
    assert_eq!(parse_args(["--unknown", "x", "--query", "y"].map(OsString::from)), Err(CliError::InvalidArguments));
}
```

Create one fixture containing a file and a directory. Test that text output contains both full paths and that JSON parses with `serde_json::from_str::<serde_json::Value>()`, reports `returned == 2`, preserves nullable metadata, and emits `kind` as `file` or `directory`.

- [ ] **Step 2: Run the focused tests and verify the red state**

Run:

```powershell
cargo test --manifest-path spikes/everything-ipc/Cargo.toml cli::tests -- --nocapture
```

Expected: FAIL because `parses_defaults` unwraps `Err(CliError::InvalidArguments)` and the renderer fixture receives `Err(CliError::RenderFailed)`.

- [ ] **Step 3: Add only the serialization dependencies**

Add target-independent dependencies without Clap or an async runtime:

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

Keep the existing `windows` dependency under `target.'cfg(windows)'.dependencies`.

- [ ] **Step 4: Implement strict parsing**

Implement `parse_args` as a single pass over `OsString` pairs. Each option may occur at most once. `--query` is required and may be an empty Everything query, while missing values, non-Unicode values, unknown options, duplicates, and a stray positional argument return `InvalidArguments`.

Use these defaults:

```rust
const DEFAULT_LIMIT: u32 = 20;
const MAX_LIMIT: u32 = 200;
const DEFAULT_TIMEOUT_MS: u64 = 1_000;
const MAX_TIMEOUT_MS: u64 = 60_000;
```

- [ ] **Step 5: Implement deterministic text and JSON rendering**

Define private serializable DTOs instead of adding serialization derives to protocol types:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonOutput<'a> {
    total: u32,
    returned: usize,
    request_flags: u32,
    sort_type: u32,
    items: Vec<JsonItem<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonItem<'a> {
    full_path: &'a str,
    file_name: &'a str,
    kind: &'static str,
    size_bytes: Option<u64>,
    modified_filetime: Option<u64>,
    attributes: u32,
}
```

Treat `attributes & 0x10 != 0` as `directory`. Text output starts with the existing summary line and then prints one tab-separated line per item: `kind`, `modified_filetime` or `-`, `size_bytes` or `-`, and `full_path`. Replace embedded CR, LF, and tab characters in text fields with a single space so each result remains one line.

- [ ] **Step 6: Export the module and run the crate tests**

Add `pub mod cli;` to `src/lib.rs` and run:

```powershell
cargo fmt --manifest-path spikes/everything-ipc/Cargo.toml -- --check
cargo test --manifest-path spikes/everything-ipc/Cargo.toml
cargo clippy --manifest-path spikes/everything-ipc/Cargo.toml --all-targets -- -D warnings
```

Expected: all commands exit `0`; existing protocol and fake-window tests remain green.

- [ ] **Step 7: Commit Task 1**

```powershell
git add spikes/everything-ipc/Cargo.toml spikes/everything-ipc/Cargo.lock spikes/everything-ipc/src/lib.rs spikes/everything-ipc/src/cli.rs
git commit -m "feat: add Everything CLI contract"
```

---

### Task 2: Wire the Existing Query2 Client and Prove a Real Query

**Files:**
- Modify: `spikes/everything-ipc/src/main.rs`

**Interfaces:**
- Consumes: Task 1 `parse_args`, `render_result`, `CliArgs`, `CliError`, and `OutputFormat`; existing `EverythingClient` and Query2 protocol types.
- Produces: the executable CLI contract from the source spec and stable process exit codes.

- [ ] **Step 1: Add failing error-classification tests**

Keep `main.rs` thin. Add a private `ProbeError` with `code()` and `exit_code()` methods, then write tests for these mappings before implementing them:

Begin with `code()` returning `E_IPC` and `exit_code()` returning `4` for every variant. This compiles while making the CLI and unavailable mappings fail deterministically.

```rust
#[test]
fn maps_expected_failures_to_stable_codes() {
    assert_eq!(ProbeError::Cli(CliError::InvalidLimit).code(), "E_LIMIT");
    assert_eq!(ProbeError::Cli(CliError::InvalidLimit).exit_code(), 2);
    assert_eq!(ProbeError::Client(EverythingClientError::ConnectionTimedOut).code(), "E_EVERYTHING_UNAVAILABLE");
    assert_eq!(ProbeError::Client(EverythingClientError::QueryTimedOut).code(), "E_QUERY_TIMEOUT");
    assert_eq!(ProbeError::Client(EverythingClientError::Protocol(ProtocolError::PayloadTooShort)).code(), "E_PROTOCOL");
}
```

- [ ] **Step 2: Run the focused test and verify failure**

Run:

```powershell
cargo test --manifest-path spikes/everything-ipc/Cargo.toml --bin everything-ipc-spike maps_expected_failures_to_stable_codes -- --nocapture
```

Expected: FAIL until the stable mapping is implemented.

- [ ] **Step 3: Replace the old fixed probe arguments with `CliArgs`**

Delete the local `ProbeArgs`, old parser, fixed `MAX_RESULTS`, and old summary-only output. Build the existing query directly from `CliArgs`:

```rust
let args = parse_args(env::args_os().skip(1))?;
let client = EverythingClient::connect(&args.instance, args.timeout)?;
let deadline = Instant::now()
    .checked_add(args.timeout)
    .ok_or(ProbeError::DeadlineOverflow)?;
let result = client.query(EverythingQuerySpec {
    search: args.query.encode_utf16().collect(),
    offset: 0,
    max_results: args.limit,
    request_flags: REQUEST_FLAGS,
    sort: EverythingSort::DateModifiedDescending,
    deadline,
})?;
println!("{}", render_result(args.format, &result)?);
```

`main` writes exactly `CODE: message` to stderr and returns the mapped exit code. Do not print partial result paths on errors.

- [ ] **Step 4: Verify invalid and unavailable command behavior**

Run:

```powershell
cargo run --quiet --manifest-path spikes/everything-ipc/Cargo.toml -- --query x --limit 0
cargo run --quiet --manifest-path spikes/everything-ipc/Cargo.toml -- --instance UiPilotCliDefinitelyMissing --query x --timeout-ms 50
```

Expected: first command exits `2` with `E_LIMIT`; second exits `3` within one second with `E_EVERYTHING_UNAVAILABLE`. Neither command panics.

- [ ] **Step 5: Run all deterministic verification**

```powershell
cargo fmt --manifest-path spikes/everything-ipc/Cargo.toml -- --check
cargo test --manifest-path spikes/everything-ipc/Cargo.toml
cargo clippy --manifest-path spikes/everything-ipc/Cargo.toml --all-targets -- -D warnings
git diff --check
```

Expected: all commands exit `0`.

- [ ] **Step 6: Run the real Everything smoke query**

With the default Everything 1.4 instance already running and DB loaded:

```powershell
$json = cargo run --quiet --manifest-path spikes/everything-ipc/Cargo.toml -- --query "*.rs" --limit 5 --timeout-ms 1000 --format json
$result = $json | ConvertFrom-Json
if ($result.returned -gt 5 -or $result.requestFlags -ne 341 -or $result.sortType -ne 14) { throw 'E_LIVE_QUERY_CONTRACT' }
$result.items | Select-Object kind, fullPath, sizeBytes, modifiedFiletime
if (@($result.items | Where-Object { [string]::IsNullOrWhiteSpace($_.fullPath) -or $_.kind -notin @('file', 'directory') }).Count -ne 0) { throw 'E_LIVE_QUERY_ITEM' }
```

Expected: command exits `0`; JSON parses; `returned <= 5`; `requestFlags == 341` (`0x155`); `sortType == 14`; every item has a non-empty `fullPath` and `kind` is `file` or `directory`.

If no default Everything instance is running, report the environment precondition as blocked and ask the user to start Everything. Do not install, launch, configure, or elevate Everything from this task.

- [ ] **Step 7: Commit Task 2**

```powershell
git add spikes/everything-ipc/src/main.rs
git commit -m "feat: print Everything query results"
```

- [ ] **Step 8: Record final evidence**

Report both Task commit hashes, the exact live-smoke command, returned count, request flags, sort type, and whether the default instance prerequisite was available. Do not include returned local paths in the report.
