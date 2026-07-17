# SystemIndex Spike Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用一个独立、可删除的 Windows Rust CLI 证明 `ISearchFolderItemFactory` 查询只使用 SystemIndex 已索引作用域，并在索引不可用或目录未被索引时不会退化为宿主文件系统遍历。

**Architecture:** Spike 位于 `spikes/systemindex`，不链接 Tauri、不修改产品命令、不向 WebView 暴露任何接口。CLI 先读取 Windows Search 状态和 Crawl Scope Manager 配置，再构造结构化字面量条件，通过 `SetScope` 绑定已索引的 `file:` 根，最后获取有限结果。服务不可用、作用域无法证明或任何 I/O 证据显示宿主枚举目录时立即 No-Go。

**Tech Stack:** Rust、`windows` crate、Windows Search COM API、PowerShell、Process Monitor 或 WPR/WPA。

**Source Spec:** `docs/superpowers/specs/2026-07-17-cross-platform-launcher-mvp-design.md` 第 6.2、6.3、10 节。

**Scope Boundary:** 本计划只产生可行性证据和 Go/No-Go 结论，不实现 `/find` UI、Tauri RPC、结果执行或性能承诺。Spike No-Go 时不得用磁盘遍历、Everything、额外索引服务或静默降级替代。

---

## Task 1: Scaffold an Isolated, Inspectable CLI

**Files:**
- Create: `spikes/systemindex/Cargo.toml`
- Create: `spikes/systemindex/src/main.rs`
- Create: `spikes/systemindex/src/lib.rs`
- Create: `spikes/systemindex/src/error.rs`
- Create: `spikes/systemindex/tests/cli.rs`
- Create: `scripts/run-systemindex-spike.ps1`

- [ ] **Step 1: Write a failing CLI contract test**

The CLI must expose only these commands:

```text
systemindex-spike status --json
systemindex-spike scopes --json
systemindex-spike query --literal <TEXT> --limit <1..100> --json
```

Write tests that reject an empty literal, a literal longer than 256 Unicode scalar values, U+0000 to U+001F control characters, a zero/over-100 limit, caller-supplied paths/scopes, and unknown flags. Assert that errors go to stderr and exit non-zero.

- [ ] **Step 2: Run the test and confirm failure**

Run: `cargo test --manifest-path spikes/systemindex/Cargo.toml`

Expected: failure because the spike crate does not exist.

- [ ] **Step 3: Create a dependency-light crate**

Use `serde`, `serde_json`, and `windows`; do not add Clap or an async runtime. Parse the three fixed command shapes with `std::env::args_os` and represent requests as:

```rust
pub enum Command {
    Status,
    Scopes,
    Query { literal: String, limit: u32 },
}

pub fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Command, SpikeError>;
```

Keep COM work behind a trait so input and fail-fast policy can be tested on any build machine:

```rust
pub trait SearchBackend {
    fn status(&self) -> Result<SearchStatus, SpikeError>;
    fn indexed_scopes(&self) -> Result<Vec<IndexedScope>, SpikeError>;
    fn query_literal(
        &self,
        literal: &str,
        limit: u32,
        scopes: &[IndexedScope],
    ) -> Result<Vec<SearchHit>, SpikeError>;
}
```

- [ ] **Step 4: Add a deterministic runner script**

`scripts/run-systemindex-spike.ps1` must build the release binary, create `artifacts/systemindex-spike/<timestamp>`, capture exact OS build/CPU/memory/storage/Search service state, run each CLI command, and preserve stdout/stderr/exit codes. It must stop if not running on Windows 11 x64.

- [ ] **Step 5: Verify parsing and build boundaries**

Run:

```powershell
cargo test --manifest-path spikes/systemindex/Cargo.toml
cargo clippy --manifest-path spikes/systemindex/Cargo.toml --all-targets -- -D warnings
```

Expected: parser tests pass; clippy exits 0.

- [ ] **Step 6: Commit**

```powershell
git add spikes/systemindex scripts/run-systemindex-spike.ps1
git commit -m "spike: scaffold isolated SystemIndex probe"
```

---

## Task 2: Enumerate Search Health and Indexed File Scopes

**Files:**
- Create: `spikes/systemindex/src/windows_search.rs`
- Create: `spikes/systemindex/src/scope.rs`
- Modify: `spikes/systemindex/src/lib.rs`
- Modify: `spikes/systemindex/src/main.rs`
- Modify: `spikes/systemindex/Cargo.toml`

- [ ] **Step 1: Write failing scope-policy tests with a fake backend**

Prove these rules:

- Only `file:` URLs explicitly included by Crawl Scope Manager can become query scopes.
- Excluded child rules remain recorded in evidence and are never widened.
- `C:\`, an unindexed directory, a network location, and a non-`file:` protocol are rejected as caller-supplied scopes.
- Stopped Windows Search, missing `SystemIndex`, COM errors, or an empty/unprovable scope set fail before a Search Folder is created.

Use a policy function with no COM dependency:

```rust
pub fn validated_file_scopes(
    status: &SearchStatus,
    rules: Vec<CrawlRule>,
) -> Result<Vec<IndexedScope>, SpikeError>;
```

- [ ] **Step 2: Run focused tests and confirm failure**

Run: `cargo test --manifest-path spikes/systemindex/Cargo.toml scope::`

Expected: compile failure because scope policy does not exist.

- [ ] **Step 3: Implement Windows Search status and scope collection**

Initialize COM in multithreaded mode and query the `SystemIndex` catalog through Windows Search APIs. Collect Crawl Scope Manager inclusion and exclusion rules as canonical URLs. Emit JSON containing:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeEvidence {
    pub catalog: String,
    pub service_running: bool,
    pub catalog_available: bool,
    pub included_file_roots: Vec<String>,
    pub exclusion_rules: Vec<String>,
}
```

Do not infer indexed roots from drive letters, environment variables, Known Folders, or successful directory access. The only accepted roots are those reported by the Search configuration API and validated by the pure policy.

- [ ] **Step 4: Instrument creation boundaries**

Add process-local counters for `search_folder_factory_created`, `scope_set`, and `search_folder_enumerated`. `status` and `scopes` must leave all three at zero. A failed precondition must also leave all three at zero. Include these counters in every JSON response and error evidence.

- [ ] **Step 5: Verify fail-fast behavior**

Run:

```powershell
cargo test --manifest-path spikes/systemindex/Cargo.toml scope::
cargo test --manifest-path spikes/systemindex/Cargo.toml
```

Expected: policy and full crate tests pass.

- [ ] **Step 6: Commit**

```powershell
git add spikes/systemindex
git commit -m "spike: prove indexed scope preconditions"
```

---

## Task 3: Build a Literal Structured Query With Explicit SetScope

**Files:**
- Create: `spikes/systemindex/src/query.rs`
- Modify: `spikes/systemindex/src/windows_search.rs`
- Modify: `spikes/systemindex/src/lib.rs`
- Create: `spikes/systemindex/tests/query_boundaries.rs`

- [ ] **Step 1: Write failing input and construction tests**

Cover single/double quotes, `*`, `?`, `%`, `_`, square brackets, backslashes, leading/trailing spaces, CJK, emoji, composed/decomposed Unicode, and the 256/257-scalar boundary. Assert accepted values are passed unchanged as literal property values rather than concatenated into WSSQL.

Test this operation order with a recording fake:

```text
check service/catalog
load validated scopes
create condition leaf
create Search Folder factory
set condition
set display name
set explicit scopes
get shell item
enumerate at most limit results
```

Any missing/reordered prerequisite must fail the test.

- [ ] **Step 2: Run focused tests and confirm failure**

Run: `cargo test --manifest-path spikes/systemindex/Cargo.toml query`

Expected: compile failure because the query module does not exist.

- [ ] **Step 3: Implement a structured literal condition**

Use `IConditionFactory::MakeLeaf` to construct exactly one `System.FileName`, `COP_VALUE_CONTAINS`, string `PROPVARIANT` leaf condition. Supply the user's text as the typed value. Never build WSSQL, AQS, SQL, or a scope expression with string interpolation.

Expose only:

```rust
pub fn execute_indexed_literal_query<B: SearchBackend>(
    backend: &B,
    literal: &str,
    limit: u32,
) -> Result<QueryEvidence, SpikeError>;
```

The real backend must create shell items for every validated scope and call `ISearchFolderItemFactory::SetScope` before obtaining/enumerating the Search Folder. If any scope conversion or `SetScope` call fails, return an error and do not enumerate.

- [ ] **Step 4: Bound result materialization**

Stop at `limit`, return only display name and canonical parsing path for evidence, and do not call `std::fs`, open returned files, inspect metadata outside Search results, follow shortcuts, or recurse into directories.

- [ ] **Step 5: Verify special characters and operation order**

Run:

```powershell
cargo test --manifest-path spikes/systemindex/Cargo.toml
cargo clippy --manifest-path spikes/systemindex/Cargo.toml --all-targets -- -D warnings
```

Expected: all boundary and ordering tests pass; clippy exits 0.

- [ ] **Step 6: Commit**

```powershell
git add spikes/systemindex
git commit -m "spike: query SystemIndex with explicit scopes"
```

---

## Task 4: Prove Service-Off and Unindexed-Directory Behavior

**Files:**
- Create: `scripts/test-systemindex-failfast.ps1`
- Modify: `scripts/run-systemindex-spike.ps1`
- Create: `spikes/systemindex/tests/failfast.rs`

- [ ] **Step 1: Write failing fake-backend tests**

Use a backend that panics if factory creation or enumeration is reached. Verify stopped service, missing catalog, empty scope set, and scope-validation failure return before the panic point. This is the deterministic regression test for the product boundary.

- [ ] **Step 2: Run the test and confirm failure**

Run: `cargo test --manifest-path spikes/systemindex/Cargo.toml failfast`

Expected: failure until all preconditions are checked before backend query construction.

- [ ] **Step 3: Implement the Windows integration harness**

`scripts/test-systemindex-failfast.ps1` must require an elevated shell before changing service state. It must:

1. Record the existing Windows Search service start type and running state.
2. Stop the service.
3. Run `query --literal uipilot-index-service-off-proof --limit 20 --json`.
4. Assert non-zero exit and counters `searchFolderFactoryCreated=0`, `scopeSet=0`, `searchFolderEnumerated=0`.
5. Restore the exact prior service state in a `finally` block.
6. Exit non-zero if restoration fails.

The script must never disable indexing permanently, delete the index, change scope rules, or run unless restoration metadata was captured first.

- [ ] **Step 4: Test an unindexed-directory sentinel**

Create a unique sentinel file in a directory confirmed absent from the included scope set. Query its exact unique name and assert zero hits. Delete the sentinel in `finally`. If the machine has no provably unindexed local directory, record the case as `not-runnable` and the Spike cannot receive Go until it is rerun on a suitable machine.

- [ ] **Step 5: Run and preserve evidence**

Run from an elevated Windows 11 PowerShell:

```powershell
cargo test --manifest-path spikes/systemindex/Cargo.toml
powershell -ExecutionPolicy Bypass -File scripts/test-systemindex-failfast.ps1
```

Expected: unit/integration tests pass; service-off exits before factory creation; unindexed sentinel returns zero results; service state is restored.

- [ ] **Step 6: Commit**

```powershell
git add spikes/systemindex scripts/test-systemindex-failfast.ps1 scripts/run-systemindex-spike.ps1
git commit -m "test: verify SystemIndex fail-fast boundaries"
```

---

## Task 5: Capture Host I/O Evidence

**Files:**
- Modify: `.gitignore`
- Create: `scripts/capture-systemindex-io.ps1`
- Modify: `scripts/run-systemindex-spike.ps1`
- Create: `docs/spikes/systemindex-evidence-protocol.md`

- [ ] **Step 1: Define the evidence protocol before running the trace**

Document an exact trace matrix:

| Case | Search state | Query target | Required result |
|---|---|---|---|
| A | Healthy | Guaranteed indexed sentinel | Sentinel appears; no host directory enumeration/content read |
| B | Healthy | Guaranteed unindexed sentinel | No hit; no host directory enumeration/content read |
| C | Stopped | Unique literal | Fail before Search Folder creation; no host directory enumeration/content read |
| D | Healthy | Quote/wildcard/Unicode fixtures | Literal semantics; no host directory enumeration/content read |

Define forbidden host-process operations as directory enumeration and content reads outside DLL/config/log loading needed to start the CLI. Windows Search service/index database I/O is allowed and must be attributed to its own process, not the spike process.

- [ ] **Step 2: Implement one supported capture path**

Prefer Windows Performance Recorder when available; otherwise use Process Monitor command-line capture. The script must:

- Build and resolve the exact spike executable path.
- Start capture before launching the process.
- Filter the report to that exact process ID and child processes.
- Preserve the raw `.etl` or `.pml` file plus an exported CSV summary.
- Stop capture in `finally`.
- Fail when no trace provider/tool is available; absence of evidence is not a pass.

- [ ] **Step 3: Run the four-case matrix**

Run:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/capture-systemindex-io.ps1
```

Expected: four raw traces and four filtered summaries are written under one timestamped evidence directory.

- [ ] **Step 4: Inspect and classify every filesystem operation**

For each case, classify the spike process's file operations as executable/DLL load, configuration/evidence output, index API side effect, directory enumeration, metadata read, or file content read. Record paths for forbidden categories. Any unexplained directory enumeration or target-file content read is a No-Go until disproved with a new trace.

- [ ] **Step 5: Commit only the protocol and scripts**

Raw traces may contain local paths and must remain gitignored. Commit the reproducible procedure, not workstation-specific raw data.

```powershell
git add .gitignore docs/spikes/systemindex-evidence-protocol.md scripts/capture-systemindex-io.ps1 scripts/run-systemindex-spike.ps1
git commit -m "test: define SystemIndex I/O evidence protocol"
```

---

## Task 6: Record the Go/No-Go Decision

**Files:**
- Create: `docs/spikes/2026-07-17-systemindex-results.md`
- Modify: `docs/superpowers/specs/2026-07-17-cross-platform-launcher-mvp-design.md`

- [ ] **Step 1: Run the complete Spike gate on the reference Windows 11 machine**

Run:

```powershell
cargo test --manifest-path spikes/systemindex/Cargo.toml
cargo clippy --manifest-path spikes/systemindex/Cargo.toml --all-targets -- -D warnings
powershell -ExecutionPolicy Bypass -File scripts/run-systemindex-spike.ps1
powershell -ExecutionPolicy Bypass -File scripts/test-systemindex-failfast.ps1
powershell -ExecutionPolicy Bypass -File scripts/capture-systemindex-io.ps1
git diff --check
```

Expected: automated tests pass and the evidence directory contains environment data, scope configuration, CLI output, fail-fast output, and four I/O traces.

- [ ] **Step 2: Write a decision report containing observed facts only**

The report must include:

- Exact Windows edition/build, CPU, memory, storage, Search service state, catalog, and `windows` crate version.
- Included `file:` roots and exclusion rules exactly as observed.
- Input fixtures, result counts, operation counters, exit codes, and elapsed times.
- Service-off restoration evidence.
- I/O classification for cases A-D with links to locally retained raw evidence identifiers.
- Each acceptance statement marked Pass, Fail, or Not Runnable with a reason.
- A single final decision: Go only when every required case passes; otherwise No-Go.

- [ ] **Step 3: Apply the decision to the frozen spec**

For Go, append the dated report path and approved query mechanism to section 6.3 without relaxing any boundary. For No-Go, mark file search blocked and state that no production implementation plan may be created until a replacement architecture receives a separate review.

- [ ] **Step 4: Review the decision boundary**

Confirm:

- No production Tauri or UI code was added by this plan.
- No disk traversal or fallback indexer exists.
- Go is supported by explicit `SetScope`, service-off, unindexed sentinel, and host-I/O evidence.
- Missing tools, missing permissions, missing unindexed scope, or ambiguous I/O evidence result in Not Runnable/No-Go, never an assumed pass.
- Machine-specific traces and paths are not committed.

- [ ] **Step 5: Commit**

```powershell
git add docs/spikes/2026-07-17-systemindex-results.md docs/superpowers/specs/2026-07-17-cross-platform-launcher-mvp-design.md
git commit -m "docs: record SystemIndex feasibility decision"
```

---

## Completion Gate

The Spike is complete only after the decision report is committed. A Go decision authorizes writing a separate production `/find` implementation plan; it does not authorize implementation by itself. A No-Go decision blocks `/find` and leaves the launcher foundation independently usable.
