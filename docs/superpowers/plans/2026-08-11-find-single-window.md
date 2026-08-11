# Single-Instance `/find` Window Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move file search into one precreated independent `find` window while preserving authenticated Everything results, ownership-safe `/find` forwarding, and process-local pin/hide behavior.

**Architecture:** Rust owns singleton-window admission, native focus transfer, readiness, pin state, and window-scoped result authorization. React routes by Tauri window label: `main` keeps launcher/settings/plugin behavior and forwards `/find`; `find` owns file search and narrow preferences. The approved design specification is the source of truth for wire formats, transition ordering, and failure rollback.

**Tech Stack:** Rust 1.96, Tauri 2.11, Windows Win32 APIs through `windows` 0.61, TypeScript 7, React 19, Ant Design 6, `@ant-design/icons` 6, Vitest 4, jsdom 29.

## Global Constraints

- Binding specification: `docs/superpowers/specs/2026-08-11-find-single-window-design.md`.
- Exactly one precreated window labelled `find`; close and hide never destroy it.
- `find` is never always-on-top. Pin only disables automatic hiding for the current process.
- Never synthesize or control the user's mouse or keyboard. Ask before running a harness that can change foreground focus.
- Preserve Everything query semantics, result limits, path identity revalidation, path-free errors, and Shell execution.
- Commands derive scope only from an exact validated caller label; callers never provide a scope.
- Rust `u64` values crossing IPC are canonical decimal strings. Local query sequence stays in `1..=Number.MAX_SAFE_INTEGER`.
- Lock order is controller then find registry. Release settings/main-registry locks before controller entry. Hold no lock across native calls, event emit, Shell execution, or async waits.
- `open_find_window` reports `forwarded` only after handoff, find `on_show`, event emit, and non-failing conditional main retirement.
- Do not stage or revert existing user changes, including `src-tauri/Cargo.toml`, `.codegraph/`, local patch scripts, and Everything runtime data.

## Execution Rules

- Every task uses TDD: add focused failing tests, confirm the intended failure, implement the minimum contract, rerun focused tests, then commit only that task's files.
- Each task receives specification compliance and code-quality review before the next task starts.
- Test variants should be table-driven where they share setup, but distinct ordering/rollback races remain separate named cases.
- Exact DTO fields and terminology are defined once in the specification sections `Frozen Terminology And Wire Contract` and `Commands And Capabilities`; implementation must match them verbatim.
- Exact state ordering and rollback are defined once in `Native Main-To-Find Focus Transfer`, `Data Flow`, and `Failure Behavior`; task summaries below do not override them.

## Tasks

### Task 1: Window-Scoped Result Authorization

**Files:** `src-tauri/src/result_registry.rs`, `src-tauri/src/commands.rs`

- [ ] Add `WindowScope::{Main, Find}` and `ResultRegistries` with independent state and one shared checked opaque-ID allocator.
- [ ] Add checked result-set generation and Rust-only `ExecutionTicket` bound to scope generation, invocation, result set, request ID, and result ID.
- [ ] Add prepared application retirement: prepare the next application-domain epoch before side effects; commit as a non-failing CAS that leaves main invocation, plugin ownership, and find scope unchanged on mismatch.
- [ ] Keep existing `begin_query`, `publish_if_latest`, `resolve`, `on_show`, and `hide_and_clear` semantics inside each scope.

**Test coverage:** concurrent main/find publication; hiding one scope does not invalidate the other; globally unique IDs; retirement superseded by newer main query; stale ticket after a newer result-set generation; checked counter exhaustion.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml result_registry::tests commands::tests::file -- --nocapture`

### Task 2: Find Admission, Readiness, Queue, and Execution State

**Files:** create `src-tauri/src/find_window.rs`; modify `src-tauri/src/lib.rs`

- [ ] Implement `NotReady`, `PreparedNotReady(token)`, `Hidden`, `Transferring`, `VisibleReady`, and `HidingForExecution(ticket)` as one Rust-owned controller state machine.
- [ ] Implement listener-first two-phase readiness: five-second preparation expiry, token supersession, idempotent ready commit/status, and no queue drain before commit.
- [ ] Implement a complete latest-only queued open transaction containing payload, prepared retirement lease, five-second deadline, and waiter. Replaced/expired/shutdown waiters terminate exactly once.
- [ ] Implement checked invocation/forward/transfer counters, confirmed focus edges plus native snapshot admission, process-local pin, and execution-hide admission closure.
- [ ] Keep the locked core synchronous and free of native I/O or waiting.

**Test coverage:** frozen state transitions; B replaced by C; prepare does not emit/wake; lost commit retry; expiry/shutdown; duplicate and contradictory focus events; two-second transfer timeout; stale/pinned execution ticket; hide success/failure with queued forward.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml find_window::tests -- --nocapture`

### Task 3: Narrow Commands and Preferences

**Files:** `src-tauri/src/commands.rs`, `src-tauri/src/settings.rs`, `src-tauri/src/find_window.rs`

- [ ] Guard every command's exact `main` or `find` label before any state, filesystem, or Shell access.
- [ ] Add main-only `open_find_window`; prepare main retirement before controller/native side effects.
- [ ] Add find-only prepare/commit/status readiness, pin, preview update, explicit hide, file search, and file execution commands using the frozen DTOs.
- [ ] Return only theme, preview, and pin initialization state to `find`. Keep theme and preview revisions independent checked decimal counters.
- [ ] Execute a file as: resolve action/ticket atomically, run existing authenticated Shell path without locks, reacquire controller then find registry, validate ticket and pin, then enter native hide only for a current unpinned ticket.

**Test coverage:** wrong labels touch no protected state; malformed input; independent field revisions; `execute A pending -> query/forward B -> A success`; pin change while Shell is pending; hide failure retires old IDs and restores admission according to queued-forward state.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml commands::tests settings::tests result_registry::tests find_window::tests -- --nocapture`

### Task 4: Native Window Lifecycle and Capabilities

**Files:** `src-tauri/src/lifecycle.rs`, `src-tauri/src/lib.rs`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/main.json`; create `src-tauri/capabilities/find.json`

- [ ] Precreate hidden `find` at 900x600 logical pixels, minimum 720x420, resizable, undecorated, and never always-on-top.
- [ ] Capture main/find focus, foreground HWND, visibility, and main topmost state. Lower main, show/focus find, and prove completion only from confirmed edges plus a fresh matching snapshot.
- [ ] Use an async one-shot with a two-second production deadline; perform no native call or wait under controller locks.
- [ ] Implement phase-specific rollback: restore captured native state before find ownership; after `on_show`, emit failure clears new find scope and keeps find hidden.
- [ ] Consume the exact handoff blur and execution-hide blur once. Restore main topmost on main refocus; later genuine main blur keeps existing launcher hide behavior.
- [ ] Prevent find destruction on close and force hide. Register managed state, window events, commands, and least-privilege non-overlapping capabilities.

**Test coverage:** exact configuration/capabilities; duplicate/delayed event rejection; contradictory HWND; handoff keeps main visible/non-occluding; pre/post-ownership rollback; main refocus/topmost; later genuine blur; expected hide blur consumed once.

**Verify:** `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` then `cargo test --manifest-path src-tauri/Cargo.toml production_wiring lifecycle::tests find_window::tests commands::tests -- --nocapture`

### Task 5: Frontend Protocol and Launcher Ownership

**Files:** `src/protocol.ts`, `src/launcher-core.ts`, `src/launcher-view.tsx`, `src/launcher.test.tsx`

- [ ] Add canonical `U64Decimal` parsing and the exact `FindForwardPayload`, `OpenFindOutcome`, `FindInitializationPrepared`, `FindReadyOutcome`, `FindPreviewPreferenceResult`, `FindThemeChanged`, and `OpenFindInput` types from the specification.
- [ ] Split `LauncherClient` and `FindClient`; neither client exposes commands outside its window capability.
- [ ] Replace embedded file mode with main-only `/find` extraction and `openFind` submission.
- [ ] Capture submission token, view epoch, query control key, and exact value. Only the current owner may clear on `forwarded` or show the fixed failure message; stale/superseded completions are inert.
- [ ] Remove embedded file state, rendering, and keyboard branches without changing launcher/settings/plugin behavior.

**Test coverage:** owned success clears; superseded is inert; late A success/failure cannot mutate edited/submitted B; `/find` forwards empty query; captured main invocation/query ownership is exact.

**Verify:** `npm.cmd test -- src/launcher.test.tsx` then `npm.cmd run build`

### Task 6: Dedicated Find Core

**Files:** create `src/find-core.ts`, `src/find-core.test.ts`; modify `src/protocol.ts`

- [ ] Register forward and theme listeners before readiness preparation. Clean partial listener registration, but retain listeners across prepare/commit response loss and retry.
- [ ] Parse narrow initialization by independent revisions; commit the exact initialization token and recover a lost commit response through idempotent retry/status.
- [ ] Accept only a valid strictly newer forward. Install a fresh invocation, reset local query sequence to zero, replace only query, and preserve category/sort/preview/pin.
- [ ] Empty query clears locally without search. Non-empty forward/edit/category changes increment the same safe local sequence; exhaustion fails the invocation closed.
- [ ] Move existing category, result list, preview, keyboard selection, stale response, preview rollback, and authenticated execution behavior from launcher core.
- [ ] Disable controls before ready and during execution; frontend does not issue a second hide after `execute_result`.

**Test coverage:** listener ordering/partial cleanup; prepare and commit response loss; event/response revision convergence; malformed/stale/duplicate/overflow forward; empty forward; query-sequence exhaustion; stale search/preview completion; pinned and unpinned execution.

**Verify:** `npm.cmd test -- src/find-core.test.ts`

### Task 7: Find View and Window Routing

**Files:** create `src/find-view.tsx`, `src/find-view.test.tsx`; modify `src/main.ts`, `src/styles.css`, `package.json`, `package-lock.json`

- [ ] Add direct `@ant-design/icons` 6.3.2 dependency and use `PushpinOutlined`, `PushpinFilled`, and `CloseOutlined` in fixed 32x32 icon-only buttons with tooltips and accessible names.
- [ ] Render search toolbar, pin/close, category sidebar, list, optional preview, status, and errors in stable responsive grid tracks.
- [ ] Set `aria-pressed` and selected styling on pin. Pinned Escape is inert; unpinned Escape requests normal hide; close always requests forced hide.
- [ ] Route startup with `getCurrentWindow().label`: `main` mounts launcher, `find` mounts `FindView` and then starts listener-first readiness. Reject unknown labels.
- [ ] Destroy only the current window's core on `pagehide`.

**Test coverage:** controls disabled before ready; accessible icon state; pinned/unpinned Escape; forced close; category/list/preview interactions; label routing; listeners registered before first readiness invoke.

**Verify:** `npm.cmd test -- src/find-core.test.ts src/find-view.test.tsx src/launcher.test.tsx` then `npm.cmd run build`

### Task 8: Real Window Harness and Acceptance

**Files:** create `src-tauri/tests/find_window_events.rs`; modify `src-tauri/Cargo.toml` only if an explicit test target is required and preserve all existing user edits.

- [ ] Add a Windows-only harness gated by `UIPILOT_RUN_REAL_WINDOW_TESTS=1`. It may use Tauri/Win32 focus APIs but must never synthesize mouse or keyboard input.
- [ ] Verify real handoff, duplicate/delayed events, timeout rollback, later genuine main blur, main topmost restoration, and one-time execution-hide blur handling.
- [ ] Run all non-foreground verification gates first.
- [ ] Before enabling the harness, tell the user: `下一步将运行 Windows 实际窗口事件测试。它不会控制鼠标或键盘，但会通过窗口 API 短暂改变前台焦点。请确认后我再运行。`
- [ ] After approval, run the harness. Then hand manual acceptance to the user; only user-operated input is allowed.

**Automated gates:**

```powershell
npm.cmd test
npm.cmd run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo check --manifest-path src-tauri/Cargo.toml
$env:UIPILOT_RUN_REAL_WINDOW_TESTS='1'
cargo test --manifest-path src-tauri/Cargo.toml --test find_window_events -- --nocapture
```

**Manual acceptance:** normal-permission `npm run tauri dev`; `/find windows`; repeated `/find system32` reuses the same window; main stays visible/non-occluding; pin/unpin blur; file/folder execution; close/reopen; `/find` opens empty without search; no duplicate find window.

## Final Checklist

- [ ] Window scope, invocation, forward sequence, query sequence, result-set generation, and execution ticket retain the specification's single meanings.
- [ ] No cross-window command or capability escalation exists.
- [ ] No lock spans native calls, event emit, Shell execution, or async waits.
- [ ] Counter exhaustion, malformed input, stale completion, response loss, queue replacement, rollback, and shutdown have focused coverage.
- [ ] Existing user changes remain untouched and unstaged.
