# Single-Instance `/find` Window Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move file search into one precreated independent `find` window while preserving authenticated Everything results, ownership-safe `/find` forwarding, and process-local pin/hide behavior.

**Architecture:** Rust owns the singleton window admission state, native focus transfer, readiness handshake, pin state, and window-scoped result authorization. React routes by Tauri window label: `main` retains application/settings/plugin behavior plus `/find` submission, while a new `FindCore` and `FindView` own file search and narrow preferences. Every asynchronous completion carries enough ownership to prove it may mutate the current UI or lifecycle.

**Tech Stack:** Rust 1.96, Tauri 2.11, Windows Win32 APIs through `windows` 0.61, TypeScript 7, React 19, Ant Design 6, `@ant-design/icons` 6, Vitest 4, jsdom 29.

## Global Constraints

- The approved contract is `docs/superpowers/specs/2026-08-11-find-single-window-design.md`; do not weaken or reinterpret it.
- There is exactly one precreated Tauri window labelled `find`; ordinary close and hide never destroy it.
- `find` is never always-on-top. Pin is process-local and controls automatic hiding only.
- Never synthesize or control the user's mouse or keyboard. Announce any real-window harness that can change foreground focus before running it.
- Preserve Everything query semantics, result limits, path identity revalidation, fixed path-free errors, and Shell execution.
- Commands derive `main` or `find` scope only from an exact validated caller label. A caller never supplies a scope.
- Cross-boundary Rust `u64` values are canonical decimal strings. Find-local query sequence is an integer in `1..=Number.MAX_SAFE_INTEGER`.
- Lock order is controller, then find registry. Release settings and main-registry locks before entering the controller. Never hold locks across native calls or async waits.
- `open_find_window` reports `forwarded` only after handoff, find `on_show`, event emit, and non-failing conditional main retirement commit.
- Existing user changes in `src-tauri/Cargo.toml`, `.codegraph/`, local patch scripts, and Everything runtime data are out of scope and must not be staged or reverted.

---

## File Map

- Create `src-tauri/src/find_window.rs`: admission state, readiness tokens, latest-only queue, focus proof, pin, and execution-hide ownership.
- Modify `src-tauri/src/result_registry.rs`: shared checked ID allocator, explicit main/find scopes, result-set generations, retirement leases, and execution tickets.
- Modify `src-tauri/src/commands.rs`: exact caller guards and main/find command split while reusing Everything and Shell adapters.
- Modify `src-tauri/src/lifecycle.rs`: native focus/foreground snapshots and main topmost restoration helpers.
- Modify `src-tauri/src/settings.rs`: narrow theme/preview snapshots and independent checked revisions.
- Modify `src-tauri/src/lib.rs`: managed state, window events, commands, and production-wiring tests.
- Modify `src-tauri/tauri.conf.json`, `src-tauri/capabilities/main.json`; create `src-tauri/capabilities/find.json`.
- Modify `src/protocol.ts`: split clients, frozen DTOs, and canonical-decimal parsing.
- Create `src/find-core.ts`, `src/find-core.test.ts`: find readiness and state machine.
- Create `src/find-view.tsx`, `src/find-view.test.tsx`: dedicated file-search UI.
- Modify `src/launcher-core.ts`, `src/launcher-view.tsx`, `src/launcher.test.tsx`: remove embedded file mode and forward `/find` with frontend ownership.
- Modify `src/main.ts`, `src/styles.css`, `package.json`, `package-lock.json`: route by window label and render the dedicated stable layout.
- Create `src-tauri/tests/find_window_events.rs`: opt-in Windows real-event harness without input synthesis.

### Task 1: Window-Scoped Result Authorization

**Files:**
- Modify: `src-tauri/src/result_registry.rs:1-702`
- Modify: `src-tauri/src/commands.rs:435-514`

**Interfaces:**
- Produces: `WindowScope::{Main, Find}`, `ResultRegistries::{main,find}`, `PreparedApplicationRetirement`, `ExecutionTicket`, `resolve_file_with_ticket`, and `retire_result_set_if_current`.
- Preserves: current `begin_query`, `publish_if_latest`, `resolve`, `on_show`, and `hide_and_clear` stale-result semantics inside one scope.

- [ ] **Step 1: Add failing scope, retirement, and ticket tests**

```rust
#[test]
fn scopes_do_not_invalidate_each_other() {
    let registries = active_registries();
    let app = publish_app(registries.main(), "main-1", 1);
    let file = publish_file(registries.find(), "find-1", 1);
    registries.find().hide_and_clear().unwrap();
    assert!(registries.main().resolve(&app.request_id, &app.result_id).is_ok());
    assert_eq!(registries.find().resolve(&file.request_id, &file.result_id), Err(ResolveError::Stale));
}

#[test]
fn retirement_commit_is_a_non_failing_cas() {
    let registries = active_registries();
    let lease = registries.main().prepare_application_retirement("main-1", 7).unwrap();
    begin_app_query(registries.main(), "main-1", 8);
    assert_eq!(registries.main().commit_application_retirement(lease), RetirementCommit::Superseded);
    assert!(current_app_mapping(registries.main()).is_some());
}

#[test]
fn ticket_is_bound_to_one_result_set_generation() {
    let registry = active_find_registry();
    let first = publish_file(&registry, "find-1", 1);
    let (_, ticket) = registry.resolve_file_with_ticket(&first.request_id, &first.result_id).unwrap();
    publish_file(&registry, "find-1", 2);
    assert!(!registry.retire_result_set_if_current(&ticket));
}
```

- [ ] **Step 2: Verify the red state**

Run: `cargo test --manifest-path src-tauri/Cargo.toml result_registry::tests -- --nocapture`

Expected: compilation fails because the scoped types and ticket APIs do not exist.

- [ ] **Step 3: Implement scoped registries with one checked allocator**

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowScope { Main, Find }

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionTicket {
    pub(crate) scope: WindowScope,
    pub(crate) scope_generation: u64,
    pub(crate) invocation_id: String,
    pub(crate) result_set_generation: u64,
    pub(crate) request_id: String,
    pub(crate) result_id: String,
}

pub(crate) struct ResultRegistries {
    main: ResultRegistry,
    find: ResultRegistry,
}

impl ResultRegistries {
    pub(crate) fn new() -> Self {
        let ids = Arc::new(AtomicU64::new(0));
        Self {
            main: ResultRegistry::with_scope(WindowScope::Main, Arc::clone(&ids)),
            find: ResultRegistry::with_scope(WindowScope::Find, ids),
        }
    }
    pub(crate) fn main(&self) -> &ResultRegistry { &self.main }
    pub(crate) fn find(&self) -> &ResultRegistry { &self.find }
}
```

Store a checked `result_set_generation` on each published mapping. Retirement preparation computes its next application-domain epoch with `checked_add`; commit only compares expected invocation/query/domain and either installs the precomputed epoch plus clears application results, or returns `Superseded`. It does not touch the main invocation, plugin domain, or find scope.

- [ ] **Step 4: Run focused tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml result_registry::tests commands::tests::file -- --nocapture`

Expected: all selected tests pass and IDs from both scopes are unique.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/result_registry.rs src-tauri/src/commands.rs
git commit -m "refactor: scope result authorization by window"
```

### Task 2: Find Admission, Readiness, Queue, and Execution State

**Files:**
- Create: `src-tauri/src/find_window.rs`
- Modify: `src-tauri/src/lib.rs:1-32`

**Interfaces:**
- Consumes: `PreparedApplicationRetirement` and `ExecutionTicket` from Task 1.
- Produces: `FindWindowController`, `AdmissionState`, `FindForwardPayload`, `OpenFindOutcome`, `FindReadyOutcome`, `QueuedOpen`, `NativeFocusSnapshot`, and transition methods used by later tasks.

- [ ] **Step 1: Write pure deterministic state-machine tests**

```rust
#[test]
fn prepare_does_not_drain_queue_and_commit_wakes_latest() {
    let now = Instant::now();
    let controller = FindWindowController::new_for_test(now);
    let b = controller.enqueue_open(open("b", lease(1)), now).unwrap();
    let prepared = controller.prepare_initialization(narrow(false, "1", "1"), now).unwrap();
    let c = controller.enqueue_open(open("c", lease(2)), now).unwrap();
    assert_eq!(block_on(b), OpenFindOutcome::Superseded);
    assert!(controller.take_startable_open().is_none());
    assert!(matches!(controller.commit_ready(prepared.token()).unwrap(), FindReadyOutcome::Ready { .. }));
    assert_eq!(controller.take_startable_open().unwrap().payload.query, "c");
    drop(c);
}

#[test]
fn execution_hide_closes_admission() {
    let controller = visible_controller(false);
    let ticket = current_ticket();
    assert_eq!(controller.begin_execution_hide(&ticket), ExecutionHideDecision::Hide);
    assert_eq!(controller.admit_search("find-1"), Err(AdmissionError::Unavailable));
    controller.complete_execution_hide(&ticket, true).unwrap();
    assert_eq!(controller.state(), AdmissionState::Hidden);
}
```

In the same module test checked-counter exhaustion; malformed canonical decimals; token replacement, five-second expiry, and idempotent commit/status; latest-only waiter timeout/shutdown; two-second transfer timeout; duplicate and contradictory focus edges; pin sampling; stale tickets; and hide failure with/without a queued forward.

- [ ] **Step 2: Verify the red state**

Run: `cargo test --manifest-path src-tauri/Cargo.toml find_window::tests -- --nocapture`

Expected: compilation fails because `find_window` is absent.

- [ ] **Step 3: Implement the locked pure core**

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AdmissionState {
    NotReady,
    PreparedNotReady { initialization_token: String, expires_at: Instant },
    Hidden,
    Transferring { transfer_id: u64, phase: TransferPhase },
    VisibleReady { invocation_id: String },
    HidingForExecution { ticket: ExecutionTicket },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FindForwardPayload {
    pub(crate) invocation_id: String,
    pub(crate) forward_sequence: String,
    pub(crate) query: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub(crate) enum OpenFindOutcome { Forwarded, Superseded }
```

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FindInitializationPrepared {
    pub(crate) initialization_token: String,
    pub(crate) theme_revision: String,
    pub(crate) theme: ThemePreference,
    pub(crate) file_preview_revision: String,
    pub(crate) file_preview_enabled: bool,
    pub(crate) pinned: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub(crate) enum FindReadyOutcome {
    Prepared { initialization: FindInitializationPrepared },
    Ready { initialization_token: String },
    Superseded,
}

Implement `prepare_initialization`, `commit_ready`, `ready_status`, `enqueue_open`, `take_startable_open`, `record_focus_edge`, `confirm_focus_snapshot`, `begin_execution_hide`, `complete_execution_hide`, `set_pinned`, `request_hide`, `expire`, and `shutdown`. Waiters are one-shot senders completed exactly once. None of these locked methods performs native I/O or waits.

- [ ] **Step 4: Run controller tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml find_window::tests -- --nocapture`

Expected: all tests pass without creating native windows.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/find_window.rs src-tauri/src/lib.rs
git commit -m "feat: add find window admission controller"
```

### Task 3: Narrow Preferences and Exact Command Boundary

**Files:**
- Modify: `src-tauri/src/settings.rs:1-900`
- Modify: `src-tauri/src/commands.rs:230-940`
- Modify: `src-tauri/src/find_window.rs`

**Interfaces:**
- Produces: `open_find_window`, `prepare_find_initialization`, `commit_find_ready`, `get_find_ready_status`, `set_find_pinned`, `set_find_preview_preference`, `hide_find_window`, and caller-first find search/execution.

- [ ] **Step 1: Add failing guard, revision, and execution-race tests**

```rust
#[test]
fn find_command_rejects_main_before_state_access() {
    let touched = AtomicBool::new(false);
    let error = prepare_find_initialization_with("main", || {
        touched.store(true, Ordering::SeqCst);
        narrow(false, "1", "1")
    }).unwrap_err();
    assert_eq!(error.code, "invalidCaller");
    assert!(!touched.load(Ordering::SeqCst));
}

#[test]
fn preview_and_theme_revisions_are_independent() {
    let revisions = PreferenceRevisions::new_for_test(4, 9);
    assert_eq!(revisions.next_preview().unwrap(), "10");
    assert_eq!(revisions.current_theme(), "4");
}
```

Also add `execute A pending -> query B -> A success`, pin change while Shell is pending, stale ticket, hide success, hide failure, and wrong-label tests proving controller/registry/settings/filesystem/Shell closures were not called.

- [ ] **Step 2: Verify the red state**

Run: `cargo test --manifest-path src-tauri/Cargo.toml commands::tests -- --nocapture`

Expected: new command and preference types are unresolved.

- [ ] **Step 3: Implement caller guards and frozen command DTOs**

```rust
fn require_label(actual: &str, expected: &'static str) -> Result<(), CommandError> {
    (actual == expected).then_some(()).ok_or_else(CommandError::invalid_caller)
}
fn require_main_window(window: &WebviewWindow) -> Result<(), CommandError> {
    require_label(window.label(), "main")
}
fn require_find_window(window: &WebviewWindow) -> Result<(), CommandError> {
    require_label(window.label(), "find")
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OpenFindInput {
    query: String,
    main_invocation_id: String,
    application_query_sequence: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FindPreviewPreferenceResult {
    file_preview_revision: String,
    file_preview_enabled: bool,
}
```

Prepare main retirement before controller entry. Snapshot narrow settings and release that lock before readiness entry. Preview persistence changes only `filePreviewEnabled` and preview revision. Theme persistence changes only theme revision and emits `find-theme-changed` to `find`.

For files, resolve `(action, ExecutionTicket)` atomically, execute the existing authenticated action without locks, then acquire controller followed by find registry. Validate/retire the exact result set, sample pin, enter `HidingForExecution`, release locks, and perform native hide. A stale or pinned ticket returns success without lifecycle mutation.

- [ ] **Step 4: Run focused backend tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml commands::tests settings::tests result_registry::tests find_window::tests -- --nocapture`

Expected: all selected tests pass.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/commands.rs src-tauri/src/settings.rs src-tauri/src/find_window.rs
git commit -m "feat: add narrow find window commands"
```

### Task 4: Native Window, Focus Transfer, Events, and Capabilities

**Files:**
- Modify: `src-tauri/src/lifecycle.rs:1-1035`
- Modify: `src-tauri/src/lib.rs:65-245`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/capabilities/main.json`
- Create: `src-tauri/capabilities/find.json`

**Interfaces:**
- Consumes: controller transitions and commands from Tasks 2-3.
- Produces: one real `find` window, native focus snapshots, phase-correct rollback, both event listeners, invoke wiring, and least-privilege capabilities.

- [ ] **Step 1: Add failing wiring and native-adapter tests**

```rust
#[test]
fn transfer_lowers_main_without_making_find_topmost() {
    let native = RecordingNativeWindows::focused_main();
    transfer_main_to_find(&controller(), &native, Duration::from_millis(20)).unwrap();
    assert_eq!(native.calls(), vec![
        Call::SetAlwaysOnTop("main", false),
        Call::Show("find"),
        Call::Focus("find"),
    ]);
    assert!(!native.was_topmost("find"));
}
```

Extend production-wiring tests to enforce exactly two configured labels, no runtime duplicate creation, both listeners, exact handlers, and non-overlapping permissions. Test pre-ownership rollback, post-ownership emit failure, duplicate edges, contradictory foreground HWND, main refocus topmost restoration, and later genuine main blur.

- [ ] **Step 2: Verify the red state**

Run: `cargo test --manifest-path src-tauri/Cargo.toml production_wiring lifecycle::tests find_window::tests -- --nocapture`

Expected: missing find configuration/capability and native adapter failures.

- [ ] **Step 3: Declare the precreated window and capabilities**

Add this configuration entry:

```json
{
  "label": "find",
  "title": "UiPilot Find",
  "width": 900,
  "height": 600,
  "minWidth": 720,
  "minHeight": 420,
  "visible": false,
  "decorations": false,
  "resizable": true,
  "alwaysOnTop": false
}
```

`main.json` grants `open_find_window` and no file-only command. `find.json` targets only `find` and grants readiness, file search/execution, pin, preview, explicit hide, plus event listen/unlisten. It grants no app search, full settings, hotkey, autostart, plugin, or launcher-hide command.

- [ ] **Step 4: Implement async focus proof and phase rollback**

Capture `main.is_focused()`, `find.is_focused()`, `GetForegroundWindow()`, visibility, and main topmost state. Listeners report only `{label, focused}`. Advance transfer only from confirmed edges plus a fresh matching snapshot. Release locks before native calls and before the two-second one-shot wait.

On confirmed main focus restore topmost. On a later unsuppressed main blur call existing `clear_and_hide`. On find close prevent destruction and request forced hide. Manage `ResultRegistries`, `FindWindowController`, and preference revisions once in `lib.rs`; register all new commands.

- [ ] **Step 5: Format and test wiring**

Run: `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`

Expected: exit code 0.

Run: `cargo test --manifest-path src-tauri/Cargo.toml production_wiring lifecycle::tests find_window::tests commands::tests -- --nocapture`

Expected: all selected tests pass and cross-window permission tests reject every mismatch.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/lifecycle.rs src-tauri/src/lib.rs src-tauri/tauri.conf.json src-tauri/capabilities/main.json src-tauri/capabilities/find.json
git commit -m "feat: wire singleton find window lifecycle"
```

### Task 5: Frontend Protocol and Launcher Submission Ownership

**Files:**
- Modify: `src/protocol.ts:1-180`
- Modify: `src/launcher-core.ts:1-1450`
- Modify: `src/launcher-view.tsx:1-650`
- Modify: `src/launcher.test.tsx:1-3140`

**Interfaces:**
- Produces: split `LauncherClient`/`FindClient`, frozen DTOs, `parseU64Decimal`, and launcher owner-checked `openFind` behavior.

- [ ] **Step 1: Replace embedded-file tests with forwarding race tests**

```typescript
it('late A success cannot clear a newer B edit', async () => {
  const a = deferred<OpenFindOutcome>()
  fake.client.openFind.mockReturnValueOnce(a.promise)
  core.editQuery('/find windows')
  const pending = core.keyDown('Enter')
  core.editQuery('/find newer')
  a.resolve({ status: 'forwarded' })
  await pending
  expect(core.getSnapshot().queryControlValue).toBe('/find newer')
})

it('superseded is inert', async () => {
  fake.client.openFind.mockResolvedValueOnce({ status: 'superseded' })
  core.editQuery('/find windows')
  await core.keyDown('Enter')
  expect(core.getSnapshot().queryControlValue).toBe('/find windows')
  expect(core.getSnapshot().status).toBe('')
})
```

Add owned forwarded clear, owned failure, late failure after B, `/find` empty extraction, and captured main invocation/application query sequence tests.

- [ ] **Step 2: Verify the red state**

Run: `npm.cmd test -- src/launcher.test.tsx`

Expected: `LauncherClient.openFind` and owner behavior are absent.

- [ ] **Step 3: Freeze DTOs and decimal parser**

```typescript
export type U64Decimal = string & { readonly __u64Decimal: unique symbol }
const U64_MAX = 18_446_744_073_709_551_615n

export function parseU64Decimal(value: unknown): U64Decimal | undefined {
  if (typeof value !== 'string' || !/^(0|[1-9][0-9]*)$/.test(value)) return undefined
  return BigInt(value) <= U64_MAX ? (value as U64Decimal) : undefined
}

export interface FindForwardPayload {
  invocationId: string
  forwardSequence: U64Decimal
  query: string
}

export type OpenFindOutcome = { status: 'forwarded' } | { status: 'superseded' }
export interface FindInitializationPrepared {
  initializationToken: string
  themeRevision: U64Decimal
  theme: 'system' | 'dark' | 'light'
  filePreviewRevision: U64Decimal
  filePreviewEnabled: boolean
  pinned: boolean
}

export type FindReadyOutcome =
  | { status: 'prepared'; initialization: FindInitializationPrepared }
  | { status: 'ready'; initializationToken: string }
  | { status: 'superseded' }

export interface FindPreviewPreferenceResult {
  filePreviewRevision: U64Decimal
  filePreviewEnabled: boolean
}

export interface FindThemeChanged {
  themeRevision: U64Decimal
  theme: 'system' | 'dark' | 'light'
}

export interface OpenFindInput {
  query: string
  mainInvocationId: string
  applicationQuerySequence: number
}
```

`LauncherClient` gets `openFind` and no file methods. `FindClient` gets only readiness/listeners, file search/execution, pin, preview, and hide.

- [ ] **Step 4: Remove launcher file mode and add submission owners**

```typescript
interface FindSubmissionOwner {
  token: number
  viewEpoch: number
  controlKey: number
  value: string
}

function ownsFindSubmission(owner: FindSubmissionOwner): boolean {
  return owner.token === findSubmissionToken && owner.viewEpoch === model.viewEpoch &&
    owner.controlKey === model.queryControl && owner.value === model.queryControlValue
}
```

Only an owned `forwarded` clears. Only an owned failure sets the fixed unavailable message. Superseded/stale outcomes do nothing. Delete embedded file state/rendering/keyboard branches while retaining application/settings/plugin behavior.

- [ ] **Step 5: Test and build**

Run: `npm.cmd test -- src/launcher.test.tsx`

Expected: all launcher tests pass.

Run: `npm.cmd run build`

Expected: TypeScript and Vite build pass.

- [ ] **Step 6: Commit**

```powershell
git add src/protocol.ts src/launcher-core.ts src/launcher-view.tsx src/launcher.test.tsx
git commit -m "feat: forward find submissions from launcher"
```

### Task 6: Dedicated Find Core and Listener-First Readiness

**Files:**
- Create: `src/find-core.ts`
- Create: `src/find-core.test.ts`
- Modify: `src/protocol.ts`

**Interfaces:**
- Produces: `createFindCore(client, maximumQuerySequence?)`, `FindCore`, `FindSnapshot`, and actions for start, forward, search, category, execution, pin, preview, hide, and destroy.

- [ ] **Step 1: Write readiness and ordering tests**

```typescript
it('registers listeners before prepare and commits the token', async () => {
  const fake = createFindFake()
  const core = createFindCore(fake.client)
  await core.start()
  expect(fake.trace).toEqual([
    'listen:find-forward',
    'listen:find-theme-changed',
    'prepare',
    'commit:init-1',
  ])
  expect(core.getSnapshot().ready).toBe(true)
})

it('accepts only newer forwards and resets local query sequence', async () => {
  const { core, fake } = await readyFindCore()
  await fake.forward({ invocationId: 'find-a', forwardSequence: '2', query: 'windows' })
  await fake.forward({ invocationId: 'find-old', forwardSequence: '1', query: 'ignored' })
  expect(core.getSnapshot().query).toBe('windows')
  expect(fake.searches[0].querySequence).toBe(1)
})
```

Add partial-listener cleanup, prepare loss/parse retry, commit loss/status retry, initialization containing no forward, independent field revisions, empty forward without search, category edits, stale search responses, query exhaustion, preview rollback, pin/execution behavior, and destruction cleanup.

- [ ] **Step 2: Verify the red state**

Run: `npm.cmd test -- src/find-core.test.ts`

Expected: module resolution fails for `./find-core`.

- [ ] **Step 3: Implement the dedicated model**

```typescript
interface FindModel {
  ready: boolean
  invocationId?: string
  lastForwardSequence?: U64Decimal
  querySequence: number
  query: string
  category: FileCategory
  sort: 'modifiedDesc'
  pinned: boolean
  themeRevision: U64Decimal
  filePreviewRevision: U64Decimal
  filePreviewEnabled: boolean
}
```

Keep controls disabled until listener registration, prepared snapshot parsing, and ready confirmation finish. Retain listeners across prepare/commit response loss. Each valid forward creates a fresh invocation, resets query sequence to zero, changes only query, and preserves category/sort/preview/pin. Empty text clears locally. Exhaustion fails the invocation closed. Move existing file list, category, keyboard, stale-response, preview, and execution behavior without changing payload semantics.

- [ ] **Step 4: Run tests**

Run: `npm.cmd test -- src/find-core.test.ts`

Expected: all find-core tests pass.

- [ ] **Step 5: Commit**

```powershell
git add src/find-core.ts src/find-core.test.ts src/protocol.ts
git commit -m "feat: add dedicated find frontend core"
```

### Task 7: Find View, Label Routing, Icons, and Styles

**Files:**
- Create: `src/find-view.tsx`
- Create: `src/find-view.test.tsx`
- Modify: `src/main.ts:1-125`
- Modify: `src/styles.css`
- Modify: `package.json`
- Modify: `package-lock.json`

**Interfaces:**
- Consumes: `FindCore` and `LauncherCore`.
- Produces: accessible `FindView`, label-routed bootstrap, and a stable dedicated layout.

- [ ] **Step 1: Add failing view and routing tests**

```typescript
it('pinned Escape is inert and pin exposes pressed state', async () => {
  const mounted = await mountFindView(pinnedCore())
  expect(screen.getByRole('button', { name: '固定搜索窗口' })).toHaveAttribute('aria-pressed', 'true')
  fireEvent.keyDown(mounted.root, { key: 'Escape' })
  expect(mounted.client.hideFind).not.toHaveBeenCalled()
})

it('close always requests forced hide', async () => {
  await mountFindView(pinnedCore())
  fireEvent.click(screen.getByRole('button', { name: '关闭搜索窗口' }))
  expect(client.hideFind).toHaveBeenCalledWith({ force: true })
})
```

Add disabled-before-ready, unpinned Escape, category sidebar, list navigation, preview toggle, and main/find bootstrap routing tests. Prove find listeners precede readiness invoke.

- [ ] **Step 2: Verify the red state**

Run: `npm.cmd test -- src/find-view.test.tsx src/launcher.test.tsx`

Expected: `FindView` and label routing are absent.

- [ ] **Step 3: Add direct icons and implement the view**

Run: `npm.cmd install @ant-design/icons@6.3.2 --save-exact`

Expected: direct dependency version `6.3.2`; the lockfile remains on 6.3.2.

Use `PushpinOutlined`, `PushpinFilled`, and `CloseOutlined` in fixed 32-by-32 icon-only Ant Design buttons with tooltips, accessible names, and `aria-pressed`. Use stable grid tracks for toolbar/sidebar/list/preview so dynamic results do not move controls.

- [ ] **Step 4: Route startup by current label**

```typescript
const label = getCurrentWindow().label
if (label === 'main') {
  mountLauncher(createLauncherCore(launcherClient))
} else if (label === 'find') {
  const core = createFindCore(findClient)
  mountFind(core)
  void core.start()
} else {
  throw new Error('unsupported window label')
}
```

Each client invokes only commands in its capability. `pagehide` destroys only the current window's core.

- [ ] **Step 5: Test and build**

Run: `npm.cmd test -- src/find-core.test.ts src/find-view.test.tsx src/launcher.test.tsx`

Expected: all selected tests pass.

Run: `npm.cmd run build`

Expected: production build passes.

- [ ] **Step 6: Commit**

```powershell
git add package.json package-lock.json src/find-view.tsx src/find-view.test.tsx src/main.ts src/styles.css
git commit -m "feat: render dedicated singleton find window"
```

### Task 8: Real Window Harness and Full Verification

**Files:**
- Create: `src-tauri/tests/find_window_events.rs`
- Modify: `src-tauri/Cargo.toml` only if an explicit test target is required; preserve every existing user edit.
- Modify: `docs/superpowers/specs/2026-08-11-find-single-window-design.md` only after automated and user acceptance pass.

**Interfaces:**
- Produces: opt-in Windows real-event regression coverage and final verification evidence; never synthesizes input.

- [ ] **Step 1: Add an opt-in real-window harness**

```rust
#![cfg(windows)]

#[test]
fn real_handoff_preserves_main_and_restores_topmost() {
    if std::env::var_os("UIPILOT_RUN_REAL_WINDOW_TESTS").as_deref() != Some(std::ffi::OsStr::new("1")) {
        eprintln!("skipped: set UIPILOT_RUN_REAL_WINDOW_TESTS=1 after user approval");
        return;
    }
    let harness = RealWindowHarness::launch_hidden_pair();
    let trace = harness.transfer_main_to_find();
    assert!(trace.main_remained_visible);
    assert!(!trace.main_topmost_while_find_focused);
    assert!(trace.find_became_foreground);
    harness.refocus_main_without_input_synthesis();
    assert!(harness.main_is_topmost());
}
```

Add duplicate/delayed event, transfer timeout rollback, later genuine main blur, and expected execution-hide blur tests. The harness may call Tauri/Win32 focus APIs but must not call input-synthesis APIs.

- [ ] **Step 2: Run all non-foreground gates**

Run: `npm.cmd test`

Expected: complete Vitest suite passes.

Run: `npm.cmd run build`

Expected: production frontend build passes.

Run: `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`

Expected: exit code 0.

Run: `cargo test --manifest-path src-tauri/Cargo.toml`

Expected: complete Rust suite passes; real-window tests opt out unless enabled.

Run: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`

Expected: exit code 0 with no warnings.

Run: `cargo check --manifest-path src-tauri/Cargo.toml`

Expected: exit code 0.

- [ ] **Step 3: Ask before changing foreground focus**

Tell the user exactly: `下一步将运行 Windows 实际窗口事件测试。它不会控制鼠标或键盘，但会通过窗口 API 短暂改变前台焦点。请确认后我再运行。`

Do not run it until the user confirms.

- [ ] **Step 4: Run the approved harness**

```powershell
$env:UIPILOT_RUN_REAL_WINDOW_TESTS='1'
cargo test --manifest-path src-tauri/Cargo.toml --test find_window_events -- --nocapture
```

Expected: all real focus-event tests pass and no input-synthesis API is invoked.

- [ ] **Step 5: Commit automated acceptance coverage**

```powershell
git add src-tauri/tests/find_window_events.rs
git diff --cached --check
git commit -m "test: cover real find window focus lifecycle"
```

- [ ] **Step 6: Hand manual acceptance to the user**

Ask the user to use their own mouse and keyboard to: start normal-permission `npm run tauri dev`; submit `/find windows`; verify main stays visible and find is focused; submit `/find system32` and verify the same window updates; test pin/unpin blur; execute file/folder results; close/reopen; verify `/find` opens empty without searching; and confirm no duplicate find window exists.

Only after every item passes, mark the specification implementation complete in a separate commit.

---

## Final Review Checklist

- [ ] Every acceptance item in the approved design maps to a named automated or manual gate above.
- [ ] `window scope`, `invocation`, `forward sequence`, `query sequence`, `result-set generation`, and `execution ticket` keep their frozen meanings.
- [ ] Cross-window caller guards run before state access and capabilities contain no overlap beyond event listening.
- [ ] No controller/registry/settings lock is held during native calls, Shell execution, emit, or async waits.
- [ ] Counter exhaustion, malformed values, stale completions, response loss, queue replacement, hide/emit failure, and shutdown have explicit tests.
- [ ] Each commit stages only its listed files; pre-existing user changes remain untouched.
