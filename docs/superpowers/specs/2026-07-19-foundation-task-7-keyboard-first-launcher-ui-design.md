# Foundation Task 7 Keyboard-First Launcher UI Design

## Status

- Date: 2026-07-20
- Document status: R1 diagnostic and R2 post-GREEN production gate No-Go; R3 Design/Plan/Code/Security Go complete
- Implementation status: Task 8 Code/Security Go at
  `16018e56486bcd4efcd1a2c81798ebc9223025e7`; local TaskCodeGo pending the revised minimal gate
- Design baseline: local `main` at
  `1dafdacbe921a25fe331633662ea1a000140dcdf`
- Task 6 dependency: Code Go and clean local integration at
  `a8626e72e97a5caa924333e6d6545efe9cd2e6d0`
- Current clean product HEAD:
  `16018e56486bcd4efcd1a2c81798ebc9223025e7`
- Release status: `ReleaseSecurityBlocked / SEC-RUNTIME-PROBE-001`

The completed R1 diagnostic and failed R2 post-GREEN gate remain design inputs, not production-pass evidence. R2 Design,
Plan, and Security Go were revoked because the target WebView2 repeatedly emitted no trusted non-composing tail after a
natural commit. R3 replaced that model, passed its production gate, and received Code/Security Go; Task 8 then received
Code/Security Go at the current product HEAD. This revision adds no product work.

## Goal And Scope

Task 7 delivers one React and Ant Design TypeScript WebView for the Windows MVP-A:

1. A keyboard-first application launcher with one search input and one result list.
2. An unframed settings view in the same WebView.
3. Exact TypeScript contracts for the eight existing Task 5 commands and the approved Task 6
   `launcher://shown` event.
4. Accessible focus, selection, status, error, long-text, and zoom behavior.
5. Vitest/jsdom tests that run without starting Tauri or changing desktop state.

Task 7 does not own application discovery, ranking, execution, activation, registry lifetime, window hide/show,
single-instance behavior, global shortcuts, tray, autostart, native dialogs, validation persistence, session cleanup, or
Windows hooks. Those remain Rust-owned by Tasks 4-6.

React, ReactDOM, and Ant Design are the only approved new UI runtime. Task 7 does not add another component framework,
state library, router, query/data-fetch library, animation package, direct icon package, plugin, slash command, file
search, macOS UI, marketing page, installer, signing, trial, or release work.

## Audited Baseline

The baseline already contains:

- Vite, TypeScript, Vitest, jsdom, and `@tauri-apps/api`; React and Ant Design are not present yet.
- A hidden, undecorated, non-resizable `720 x 420` window labelled `main`.
- `src/main.ts`, `src/styles.css`, and `index.html` with a minimal search input shell.
- Task 5's exact eight production commands, common main-caller guard, `SearchResponse`, `SettingsView`,
  `UserSettingsUpdate`, `ExecuteOutcome`, `ExportOutcome`, and fixed `CommandError` codes.
- Capability for those eight commands plus event listen/unlisten. No window, shortcut, tray, shell, process,
  filesystem, or HTTP permission is available to the WebView.

The approved Task 6 design and plan freeze `launcher://shown`, the listener-before-`load_settings` readiness handshake,
the fixed launcher/settings targets, and preservation of the `save_settings` outer `settings` argument. Task 6 has not
implemented that contract on the Task 7 baseline. Its approved plan is a compatibility contract, not production
evidence; real wiring still waits for Task 6 Code Go and approved local integration.

## Chosen Approach

### A. Independent ownership core, thin React/AntD view, then real adapter (chosen)

`src/launcher-core.ts` owns explicit protocol, invocation, async, search, IME-classification, and settings state behind
the exact injected client. It imports neither React nor Ant Design. `src/launcher-view.tsx` subscribes to immutable core
snapshots and renders them with React and the approved Ant Design components; it cannot invoke Tauri itself.

`src/native-input.ts` is the production trust adapter. Its native listeners non-injectably inspect the real event
object before emitting a small classified record to the core. `src/main.ts` later owns the only real Tauri client and
React root after the Task 6 and WebView2 hard gates.

This keeps protocol/security ownership independent of presentation while honoring the updated Ant Design requirement.

### B. Let AntD AutoComplete/Select own search state (rejected)

Those controls introduce hidden option identity, keyboard, popup, active-descendant, filtering, and IME behavior that
conflicts with the frozen registry and accessibility contract. The default is controlled AntD `Input` plus an explicit
result list. AutoComplete/Select remains forbidden unless a new design proves every exact ownership and DOM boundary.

### C. React context/store or direct Tauri calls in components (rejected)

The UI has one small state owner. A state library, router, query library, generic RPC layer, or component-level Tauri
calls would duplicate the core, obscure exact command arguments, and make listener ordering harder to prove.

## Ownership And File Boundary

The future Task 7 product allowlist is frontend/package-only:

```text
package.json
package-lock.json
tsconfig.json
src/launcher-core.ts
src/native-input.ts
src/launcher-view.tsx
src/launcher.test.tsx
src/main.ts
src/protocol.ts
src/styles.css
```

The existing `index.html` already has the correct language, app root, stylesheet, and `src/main.ts` entry and remains
byte-identical. `vite.config.ts` also remains byte-identical: Vite already compiles imported `.tsx` for production, so
the plan may add only `"jsx": "react-jsx"` to `tsconfig.json`. `src/main.ts` can use `createRoot` plus
`createElement(LauncherView, ...)` and does not require an entry rename. Do not add `@vitejs/plugin-react` for Fast
Refresh or development convenience; if the approved production build cannot compile this exact path, stop for review.

Task 6 exclusively owns Rust lifecycle, `Cargo.toml`, `lib.rs`, `commands.rs`, `settings.rs`, and related tests. Task 7
must not delete Task 6 lint exceptions, wire lifecycle callbacks, or change any command/event producer.

## Dependency And Supply-Chain Boundary

Candidate direct production versions are frozen for design review:

```text
dependencies (exact values, no range prefix)
react     19.2.7
react-dom 19.2.7
antd      6.5.1

devDependencies (exact values, no range prefix)
@types/react     19.2.17
@types/react-dom 19.2.3
```

The exact minimum TypeScript additions for this TSX path are direct development dependencies
`@types/react 19.2.17` and `@types/react-dom 19.2.3`, plus the one `tsconfig.json` JSX setting above. React, ReactDOM,
AntD, and both type packages report MIT licenses. AntD 6.5.1 directly depends on `@ant-design/icons ^6.3.2`; Task 7
does not declare or import `@ant-design/icons` directly, but the implementation plan must report its exact resolved
transitive version rather than claim the tree contains no icon package. AntD also declares
`@rc-component/motion ^1.3.3`; Task 7 adds no direct animation dependency, imports no motion package, and disables AntD
motion in `ConfigProvider`, but the complete resolution must identify this transitive package rather than omit it.

Before Plan Go or TDD, a disposable scripts-disabled resolution in a temporary copy must freeze and review:

- exact direct and complete transitive additions relative to baseline `package-lock.json`;
- every resolved version, integrity value, package source, license, and lifecycle-script declaration;
- `npm audit` results for the full tree and production-only tree, both with zero high or critical findings;
- peer-dependency consistency for React 19.2.7;
- proof that the existing Vite config builds TSX without a React plugin.

Both resolutions use exactly Node `v22.21.1` and npm `10.9.4`. Their evidence must show identical effective values for
`registry=https://registry.npmmirror.com/`, `package-lock=true`, command-forced `package-lock-only=true` and
`ignore-scripts=true`, `legacy-peer-deps=false`, `strict-peer-deps=false`, `install-links=false`, `workspaces=null`, and
empty `omit`/`include`, and must reject any unapproved `npm_config_*` or project/user config that changes resolution.

The evidence records baseline and candidate `package.json`/`package-lock.json` SHA-256 hashes, proves the two fresh
scripts-disabled resolutions are byte-identical, and runs `npm ls --all`. For lockfile v3, `packages[""]` is authenticated
separately against the exact root `package.json` name, version, dependency, and devDependency maps; it is not required to
have `resolved` or `integrity`. Every non-root, non-link registry package entry must have the approved resolved source
and `sha512` integrity. Link, `file:`, workspace, Git, or any other non-registry package type is unapproved for Task 7 and
fails review rather than bypassing source/integrity checks.

The approved plan must copy those results, exact install/lock commands, config evidence, and hashes. Product dependency
preparation uses exact versions and scripts disabled; implementation uses
`npm ci --ignore-scripts --no-audit --no-fund`. If any required package needs an install lifecycle script,
integrity/source/license cannot be authenticated, audit has a high/critical finding, or the resolved tree differs, stop
for dependency/security review. Do not use `--force`, `--legacy-peer-deps`, change the baseline registry/source policy,
add overrides, or add postinstall workarounds.

The baseline lock currently resolves through its existing `registry.npmmirror.com` source, whose audit endpoint returns
`404 NOT_IMPLEMENTED`. A read-only `npm audit --registry=https://registry.npmjs.org/` is therefore the one explicit
advisory-query exception; it must not rewrite package sources, npm config, package files, or the lock. On this design
baseline, both full-tree and `--omit=dev` official audit responses report `high: 0` and `critical: 0`. The same two
successful machine-readable responses must be refreshed against the candidate lock before Plan Go; failure to obtain an
audit response is not a pass.

The current CSP already permits self-hosted scripts and inline styles required by AntD CSS-in-JS. It must remain
byte-identical, as must `withGlobalTauri: false`, capabilities, permissions, Rust/Cargo, build scripts, and security
probe/config/scripts. AntD receives no Tauri client. Any need to change one of those trust inputs stops for a new
security design review.

## Frozen TypeScript Protocol

### Data transfer objects

```ts
export interface ResultItem {
  resultId: string
  title: string
  subtitle?: string
  icon?: string
}

export interface SearchResponse {
  requestId: string
  items: ResultItem[]
}

export interface AppAliasTarget {
  appId: string
  displayName: string
  icon?: string
  aliases: string[]
}

export interface SettingsView {
  hotkey: string
  autostart: boolean
  researchId?: string
  applications: AppAliasTarget[]
}

export interface UserSettingsUpdate {
  hotkey: string
  autostart: boolean
  researchId?: string | null
  aliases: Record<string, string[]>
}

export type ExecuteOutcome =
  | { status: 'launchRequested' }
  | { status: 'activationRequested' }
  | { status: 'activationRefusedLaunchRequested'; message: string }

export type ExportOutcome = { status: 'cancelled' } | { status: 'exported' }

export type CommandErrorCode =
  | 'invalidCaller'
  | 'staleRequest'
  | 'unknownResult'
  | 'applicationEntryUnavailable'
  | 'settingsFailed'
  | 'validationFailed'
  | 'windowFailed'
  | 'scanFailed'
  | 'scanWorkerFailed'
  | 'mainThreadDispatchFailed'
  | 'exportFailed'
  | 'exportWorkerFailed'

export interface CommandError {
  code: CommandErrorCode
  message: string
}

export type ShowTarget = 'launcher' | 'settings'
export type LifecycleNotice = 'settingsFailed' | 'validationFailed'

export interface LauncherShown {
  invocationId: string
  target: ShowTarget
  notice: LifecycleNotice | null
}
```

`SettingsView.researchId` is absent when Rust holds `None`; it is never JSON `null`. `UserSettingsUpdate` accepts
missing or `null`, but the Task 7 form emits an absent property for an empty research ID. `LauncherShown.notice` is
different: the approved Task 6 Rust DTO has no `skip_serializing_if`, so no notice is JSON `null`, not an omitted field.

Only `SettingsView.applications[].appId` and `UserSettingsUpdate.aliases` keys contain `appId`. Search and execution do
not accept or return it. `appId` is kept in core memory solely to build settings updates; it is never displayed,
used in DOM IDs/data attributes, logged, or passed to an action command.

### Exact command calls

| Command | Exact frontend argument object | Result |
|---|---|---|
| `search_apps` | `{ query, invocationId, querySequence }` | `SearchResponse \| null` |
| `execute_result` | `{ requestId, resultId }` | `ExecuteOutcome` |
| `load_settings` | no argument object | `SettingsView` |
| `save_settings` | `{ settings: UserSettingsUpdate }` | `void` |
| `rescan_apps` | no argument object | `void` |
| `export_validation_data` | no argument object | `ExportOutcome` |
| `clear_validation_data` | no argument object | `void` |
| `hide_launcher` | no argument object | `void` |

The outer `save_settings` key is exactly `settings`, matching the current Task 5 command and the Task 6 approved
frontend-compatibility requirement. It must not be renamed to `input`, flattened, or wrapped in a generic payload.

No command argument may contain a path, shortcut, executable, PID, HWND, shell operation, command line, working
directory, raw action, or window target. Task 7 never imports a Tauri window API and never calls `hide()` directly.

## Client Boundary And Startup Order

The core consumes one narrow client with one method per approved operation and one event subscription method. The
interface exists because the real Tauri adapter and the Vitest fake are both required consumers; it is not a generic
RPC abstraction. React/AntD components never receive this client and can only render snapshots or send approved local
intent/classified-input records to the core.

Startup order is fixed:

1. Create the core in a non-ready state.
2. Mount `ConfigProvider` -> AntD `App` -> `LauncherView` into the existing `#app` root.
3. Await the view-ready signal emitted only after the controlled AntD Input exposes its real `HTMLInputElement` and all
   production native input/composition listeners are attached.
4. `await` successful registration of the one `launcher://shown` listener.
5. Only after registration resolves, call `load_settings` once.
6. Store the returned settings projection without synthesizing a view show.

If React mount/native listener binding or Tauri listener registration rejects, Task 7 must not call `load_settings`.
Calling it would allow Task 6 to mark the frontend ready and drain an event that no listener can receive. The core
remains in a fixed initialization error state; it does not invent a ready command or polling fallback.

`load_settings` may cause an already pending Task 6 target to emit before its Promise settles. The installed listener
must process that event immediately. A settings target received during the load renders a loading settings view and is
filled when the same Promise resolves. If settings loading fails after the listener was installed, launcher search can
still operate after a launcher event; the settings view shows a fixed load failure and an explicit retry that calls the
same idempotent `load_settings` command.

The fake event source and thin-view tests prove this ordering before Task 6 finishes. They do not claim real lifecycle
integration. The real adapter, real event smoke, and any startup/readiness E2E wait for the exact post-Task-6 hard gate.

## React And Ant Design View Boundary

The thin view uses React `useSyncExternalStore` against the core's `getSnapshot`/`subscribe`; it does not mirror protocol
ownership in React context or a second reducer. Its approved AntD value imports are exactly `ConfigProvider`, `App`,
`Input`, `Form`, `Checkbox`, `Button`, `List`, `Alert`, `Spin`, and `theme`; public type-only imports may describe refs.
AntD is used where it does not take ownership from the core:

`getSnapshot` returns the exact same `Object.is` reference while core state is unchanged. Each real logical mutation
publishes one new immutable snapshot before notifying each current subscriber exactly once; a no-op, invalid, untrusted,
or stale input creates no snapshot and sends no notification. The `getSnapshot` and `subscribe` function identities are
stable for the core lifetime, and every returned unsubscribe function is idempotent.

- `ConfigProvider` and AntD `App` provide theme/context only. ConfigProvider disables AntD motion globally.
- A controlled AntD `Input` renders `queryControlValue` and receives combobox/listbox/active-descendant attributes. Its
  underlying `InputRef.input` must be the exact native element authenticated by `src/native-input.ts`.
- AntD `List` or unframed layout primitives render explicit result rows, but Task 7 supplies the listbox/option IDs,
  wrap navigation, selected index, visibility seam, and `requestId`/`resultId` mapping.
- AntD `Form`, `Input`, `Checkbox`, and `Button` render settings. The core issues monotonic, view-local `ControlKey`
  values that remain stable for each control's lifetime and are used for React keys, labels, field names, and DOM IDs;
  none derives from `appId`. `Form.Item` does not receive an `appId`-based `name`, and local intent closures map the
  control key back to the core-private ID.
- AntD `Alert` and `Spin`/button loading states may present fixed local feedback and busy state inside the one approved
  live region. Raw thrown values never reach AntD props.
- Clear remains an inline two-step confirmation built from unframed feedback and text buttons. `Modal`, `Popconfirm`,
  portals, and popup focus traps are not used by default because the frozen focus/Narrator behavior is already explicit.

Do not use AntD `AutoComplete` or `Select` for launcher search. Do not use Card/nested-card composition, remote assets or
fonts, AntD notification/message globals, motion as product behavior, or direct icon imports. A local CSS generic app
mark remains decorative. AntD tokens plus minimal `src/styles.css` own the dense 720 x 420 layout, forced colors,
long-text wrapping, and zoom behavior.

## R1/R2 Runtime Evidence And R3 Boundary Candidate

The completed Pass A on Windows `10.0.26100` / WebView2 `150.0.4078.83` observed natural Microsoft Pinyin input in both
launcher and settings. A same-IME trusted non-composing `insertText` tail and a later independent same-value ordinary
`insertText` were identical across every permitted event-native non-text field. Natural cancel also did not prove a
correct committed-value restore from the trusted draft. Pass B correctly did not run; the product recovered clean at
`28f058be94d4fadb0b490b08f4bb5f99a77c08f0`.

The R2 post-GREEN gate then proved that natural launcher and settings commits can end after trusted start and trusted
composing input with only an untrusted `compositionend`; no trusted non-composing tail follows. R2 therefore leaves the
trusted draft uncommitted and is revoked. The four-file R2 diff was recorded and restored without a commit; the product
recovered clean at `28f058be94d4fadb0b490b08f4bb5f99a77c08f0`.

R3 does not classify a tail. It accepts one zero-payload boundary only for an active binding established by a trusted
same-target start. Exact-value state idempotence makes boundary-then-tail and ordinary-input-then-boundary converge on one
logical commit. Timer, microtask, adjacency/order/delay, `inputType` classification, value matching as event correlation,
suppression markers, tombstones, end text/value, dependency, command, permission, plugin, generic event framework, and a
Rust/Win32 IME bridge remain rejected.

## Native DOM Trust Adapter

The clean recovery production `src/native-input.ts` remains authoritative until R3 receives written Go. R3 proposes
exactly:

```ts
type ClassifiedTextRecord =
  | { kind: 'compositionStart'; control: ControlKey }
  | { kind: 'compositionInput'; control: ControlKey; value: string; inputType: string }
  | { kind: 'ordinaryInput'; control: ControlKey; value: string; inputType: string }
  | { kind: 'compositionBoundary'; control: ControlKey }
```

Each actual binding owns one local active flag. Only a real trusted `CompositionEvent` `compositionstart` from the same bound control and target sets it and emits the zero-text start. A trusted same-target `InputEvent` emits
`compositionInput` while `isComposing === true`; otherwise it
atomically clears an active flag, if present, and emits `ordinaryInput`. Input values come only from those trusted input
events; `inputType` is payload metadata, never a classifier.

A same-target `CompositionEvent` `compositionend` may emit `compositionBoundary` regardless of `isTrusted`, but only while
that binding's active flag is set by its trusted start. The adapter clears the flag before emission. The boundary carries
no data/value, never reads DOM value or CompositionEvent data, and cannot create ownership. Duplicate, no-start, wrong-target,
post-unbind, retired, and replaced-control ends emit nothing. Unbind clears the flag before removing listeners
idempotently; view cleanup then calls `retireControl`. A trusted ordinary input that clears the flag makes a later end
zero-effect.

React SyntheticEvent handlers remain inert and cannot claim text ownership. There is no injected trust bit or reusable
IME framework.

## Launcher State And Data Flow

One framework-independent core owns only in-memory UI state:

```ts
interface LauncherState {
  view: 'launcher' | 'settings'
  viewEpoch: number
  invocationId?: string
  query: string
  queryControlValue: string
  querySequence: number
  requestId?: string
  items: ResultItem[]
  selectedIndex: number
  searchPending: boolean
  executePending: boolean
  shownNotice?: string
  status: string
}
```

`query` and committed settings form values remain the last core-committed values; control drafts remain temporary. The
core owns at most one composition owner `{ control, viewEpoch, invocationId, generation, lastTrustedDraft }`, initialized
from committed core state by `compositionStart` and updated only by matching `compositionInput`. A matching boundary
clears that owner first, then commits only its stored `lastTrustedDraft`; it never reads the DOM. A matching ordinary
input clears ownership first and uses its trusted input value. No-owner, stale, retired, replaced, and duplicate records
are zero-effect.

After ownership clears, commit first synchronizes the control draft to its trusted/stored value. If that value equals the
committed value, it makes no committed-state mutation, search sequence/call, or settings mutation. When the control already
shows that value, as in boundary-then-tail or a later same-value input, the snapshot reference also stays unchanged. When
an unfinished draft differs, restoring the control may publish only that visible restoration once. Therefore boundary
followed by a same-value tail, ordinary input followed by a boundary, and a later same-value ordinary input remain one
logical commit. Rerunning an unchanged launcher query is an explicit Enter/command concern, not an input-event side
effect. A different later ordinary value is one normal edit and applicable search/settings mutation. Uncommitted text is
never a shown query, settings update, or command input.

The React `onChange` required by a controlled input is inert for ownership and cannot emit an edit; only the
authenticated native adapter can send a classified record. Settings form state and the last loaded `SettingsView` are
owned by the same core, not AntD Form state, React context, or a second store. No Task 7 state is written to local storage
or another file. Suppression/tombstones do not exist; epoch/invocation/generation and exact control ownership reject stale
records.

Every valid shown event increments `viewEpoch`. Every asynchronous UI operation captures the epoch, expected view,
invocation ID where applicable, and its own operation token. A completion may update visible state, replace settings,
start a follow-up load, announce status, or restore focus only while all captured ownership values still match. A new
shown event invalidates those UI continuations but does not cancel, retry, or compensate a Rust command that is already
running.

The initial readiness `load_settings` is the one explicit exception to epoch-scoped data hydration: it starts only after
the listener is installed and may itself cause the first shown event before returning. Its latest successful result may
populate the in-memory settings projection across that first event, but it applies focus/status only to the currently
active view. Explicit load retries and every post-save/post-rescan load obey the normal epoch rule.

### `launcher://shown`

For every valid event the core:

1. Invalidates every composition owner from the prior epoch and restores each draft to its last committed value before
   automatic search. Records from a retired/replaced old binding are stale; a later trusted non-composing input from the
   still-current binding is a new ordinary edit even if the browser produced it as an IME tail.
2. Stores the new `invocationId` and target.
3. Resets `querySequence` to `0`.
4. Invalidates pending search/execution UI continuations.
5. Preserves only the committed `state.query` but clears request, results, selection, and transient errors because their
   Rust mapping belongs to the previous invocation.
6. Switches the same WebView to launcher or settings.
7. Announces a fixed lifecycle notice when non-null.
8. On launcher target, focuses the search input and selects its full contents after the launcher DOM is active.
9. On settings target, focuses the settings heading so Narrator announces the view before normal Tab navigation.
10. On launcher target only, if committed `state.query` is non-empty, increments the reset sequence to exactly `1` and
    issues one search owned by the new invocation/epoch. Empty committed query makes zero Rust calls.

A late composing record without a current owner, or any record from a retired/replaced binding, must make zero sequence,
state, focus, or Rust changes. A trusted non-composing input from the current binding is an ordinary edit, including when
its value equals committed text.

On a launcher target, a non-null lifecycle notice has display priority and does not consume a pending
activation-refused notice. The first later launcher target with `notice: null` displays and consumes that one-shot
activation-refused notice. A settings target never displays or consumes it. The chosen text is stored separately as
`shownNotice`: auto-search may update pending state and hydrate results, but it cannot replace the notice. The live
region renders the notice before any auto-search result/error text. The first later explicit query edit or user command
clears the shown-level notice and returns status ownership to that interaction.

An invalid event payload is ignored and never creates an invocation or command call. Task 7 never creates an
`invocationId` itself.

### Search

- The input is disabled until a valid launcher event supplies an invocation ID.
- Every ordinary, non-composing input edit runs one edit path: it commits the exact control text to `state.query` and
  increments the in-memory `querySequence`, including an edit to empty text. Empty text immediately clears request,
  results, selection, pending state, and ordinary status without calling Rust; incrementing still invalidates an
  in-flight response.
- Every non-empty ordinary edit calls only `search_apps` with the exact input text, current invocation ID, and new
  sequence. There is no debounce, trim, recommendation route, slash command, or secondary search provider.
- A search start immediately clears the previous request/results/selection and ordinary `status`, then sets
  `searchPending`; old mapped IDs are never left executable while a newer query is pending. `shownNotice` remains
  independent and is not cleared by an automatic search start.
- A response or error commits only when captured epoch, invocation ID, sequence, search token, and exact query text
  still equal the current committed `query` and rendered `queryControlValue`. Older responses, older errors, and older
  `null` are zero-effect, including no pending-state release.
- A current matching Rust `null` releases `searchPending` but does not replace request/results/status. A current error
  releases pending and announces only its fixed local error text. A current non-null response releases pending and
  replaces the result set. No items shows `未找到应用`; empty input shows no recommendation or instructional empty
  state. Because search start already cleared ordinary status, a current `null` leaves it empty; only an independent
  shown-level notice may remain first in the live region while its automatic search settles.
- Non-empty results select index `0`. ArrowDown and ArrowUp wrap; no-results arrows do nothing. Focus stays in the
  combobox. After every keyboard selection change, Task 7 calls an injected visibility seam whose real implementation
  is equivalent to `activeOption.scrollIntoView({ block: 'nearest' })`; jsdom asserts the seam call without layout.
- A trusted start retires the previous search token without calling Rust and initializes the owner from committed text.
  A matching trusted composing input updates only the temporary draft and makes zero sequence/search/settings commits.
  Returning to an old value cannot revive the retired response.
- A matching zero-payload boundary clears ownership first and commits only `lastTrustedDraft`. A trusted non-composing
  input clears matching ownership first and commits only its trusted input value. Either route may finish the session;
  the later route is zero-effect when ownership is gone and the value is already committed.
- Exact committed-value input has no sequence/search/settings mutation. It preserves snapshot identity when visible text
  already matches; otherwise it publishes only the draft restoration once. A different later ordinary value takes the
  normal edit path once. Task 7 does not classify an IME tail versus a later genuine edit, and unchanged query rerun
  remains an explicit Enter/command action.
- If no boundary or trusted non-composing input arrives before a valid shown event, unbind, control replacement, or
  `retireControl`, that transition discards draft, restores committed value, and makes zero search/settings commit.
- A trusted composing input without a matching owner, and every no-owner/stale/retired/replaced-control record, is
  zero-effect. The only untrusted exception is the correlated zero-payload same-target end while its binding-local active
  flag exists. End `data`, DOM end value, timing, microtasks, event order/distance, suppression, and tombstones are never
  state-machine inputs.
- Calling the composing `keyDown(Enter | ArrowUp | ArrowDown | Escape, true)` handler itself does not search, navigate,
  execute, hide, or mutate the view. A later browser-emitted trusted non-composing InputEvent is a separate ordinary edit:
  `deleteContentBackward` after Escape commits/searches once in launcher and commits locally once with zero Rust calls in
  settings. It is not attributed to or suppressed as an Escape side effect.

### Execute and hide

- Enter with a current selection invokes only `execute_result` with the current `requestId` and selected `resultId`.
- While execution is pending, repeated Enter is ignored so the frontend cannot intentionally duplicate one action.
- A success never calls a window API or `hide_launcher`; Rust owns registry invalidation and hide.
- Launch and activation success need no visible completion state because Rust hides the launcher.
- `activationRefusedLaunchRequested` always sets one process-local boolean notice after the trusted success returns,
  even if a newer shown event has already invalidated the execution's UI epoch. It never mutates that newer view. The
  first later launcher event eligible under the notice-priority rule displays and consumes the fixed message. Multiple
  late refusal outcomes coalesce into the same one-shot notice; they do not form a queue.
- A current execution error keeps the window and query state, restores Enter availability, and announces a fixed
  frontend message. An error from an invalidated execution epoch is ignored.
- Launcher Escape, settings Escape, and the settings close control share one `hidePending` owner and exact token. A
  non-composing Escape first prevents the input's native Escape behavior, then invokes only `hide_launcher`. While a
  same-epoch hide is pending, any of the three triggers makes zero second calls. Current hide success relies on Rust,
  releases its exact pending token, and makes no local view mutation. Current hide rejection releases pending, keeps
  the exact view/query/focus, and announces the fixed code/fallback text. A stale hide success/error performs no status,
  view, or focus update and cannot release newer ownership.

Task 7 does not enter settings locally; the only settings entry is a valid `launcher://shown` settings target. The
visible settings control has visible text and accessible name `关闭`, invokes only `hide_launcher`, and performs zero
local view switch. The user reopens launcher through a normal Task 6 Launcher request, which emits a new launcher
invocation and records `LauncherInvoked` before Task 7 can search or execute. Adding a local launcher target or another
command requires new cross-task design review.

## Settings View

Settings is an unframed full-view layout in the same WebView, not a dialog, card, route, or second window. It contains:

- `关闭` command that only invokes `hide_launcher`.
- AntD Research ID `Input` forwarding native `maxlength=64` and pattern `[A-Za-z0-9_-]{1,64}` while still allowing
  empty.
- AntD hotkey `Input`; Task 7 does not parse or normalize shortcut syntax.
- AntD autostart `Checkbox`.
- Application alias editor.
- Save, reload settings, rescan, validation export, and validation clear commands.

Each current application is rendered in the exact `SettingsView.applications` order. Core identity is `appId`, but the
DOM exposes only display name, optional safe presentation, and alias fields. Same-name applications receive a
display-only ordinal suffix such as `Name (1)` and `Name (2)`; the opaque IDs remain hidden and independently key each
alias vector.

The alias editor uses one labelled text input per alias plus add/remove commands. An application with no aliases still
offers one blank input so the first alias can be created. Save preserves non-empty values in order and omits fields
whose value is exactly empty; it does not split on commas, parse paths, deduplicate, or use display names as keys.

Save constructs aliases for every application currently in the loaded projection, including empty vectors, and calls
only:

```ts
save_settings({ settings: { hotkey, autostart, researchId, aliases } })
```

Temporarily absent applications are not present in `SettingsView`, so Task 7 neither displays nor invents their IDs.
The submitted map contains only the complete current projection; Task 5's existing store remains solely responsible
for preserving absent aliases and all `useCounts`. Task 7 adds no client-side shadow copy of either hidden field.

After save succeeds, Task 7 marks the projection stale and calls `load_settings` once to receive Task 6's canonical
hotkey and the current application projection. If reload fails, it reports that save succeeded but display refresh
failed and keeps the submitted form read-only. A save error does not reload or discard edits; because Task 6 may have
persisted before runtime cleanup failed, it also marks the projection stale and uses the fixed restart-aware error text.

Rescan calls `rescan_apps`, then marks the projection stale and calls `load_settings` once on success. The form is
replaced and made editable only after both succeed; rescan failure keeps the previous editable projection, while a
post-rescan load failure keeps the previous form read-only. Export calls only `export_validation_data`; cancelled is a
neutral status and exported is a success status. Clear uses an inline two-step confirmation, not `window.confirm`, so
modal focus loss cannot be mistaken for launcher focus loss; confirmation calls only `clear_validation_data`.

Only one settings operation is active at a time. Relevant controls are disabled while it runs, and focus returns to the
initiating control only when its captured epoch/view/token still owns the UI. A shown event does not clear the global
operation token or allow a second settings operation. When a stale save/rescan completion may have changed backend
state, the core releases only that exact operation token and records an internal `settingsNeedsReload` flag; it
performs no follow-up load, form replacement, status write, or focus restoration. Only controls permitted by the reload
flag are re-enabled. A later settings entry or the explicit reload command resolves that flag through one current-epoch
`load_settings` call. Late export, clear, and hide completions likewise release only their own busy ownership and
perform no other visible UI continuation. Backend membership and validation remain authoritative; Task 7's native form
constraints are usability checks, not a security boundary.

`settingsNeedsReload` is fail-closed and is cleared only by a successful current-epoch `load_settings`. While set, all
editable settings fields, alias add/remove/edit, Save, and Rescan remain disabled; `关闭`, reload settings, export,
and validation clear remain available. This prevents an old projection from submitting an incomplete aliases map after
a rescan makes a previously absent application current. Task 7 never tries to merge hidden aliases itself.

The current backend does not define a safe URL grammar for `icon`. Task 7 keeps the optional field in the protocol but
does not navigate or fetch it. MVP rendering uses a local CSS generic application mark with `aria-hidden=true`; adding
real icon rendering requires a separately frozen non-path format.

## Visual And Responsive Contract

- The launcher is a quiet edge-to-edge work surface: search row, status region, and separated result rows. No hero,
  nested cards, gradient, decorative blobs, or animation.
- The result area uses remaining height and scrolls. Settings uses one vertical scroll region.
- Result title, subtitle, application names, notices, and errors use `min-width: 0`, `overflow-wrap: anywhere`, and
  multi-line rows. Task 7 does not truncate the accessible text or let long text overlap controls.
- Controls keep stable heights and visible focus rings. Text uses rem-based sizes, never viewport-scaled fonts or
  negative letter spacing.
- `ConfigProvider` uses AntD's default/dark algorithm from `prefers-color-scheme` and only minimal local token/CSS
  adjustments for density and contrast; it does not create a branded theme. Error meaning includes text and does not
  rely on color. `forced-colors` keeps borders and focus visible.
- At 100%, 150%, and 200% WebView zoom, the single fixed window may scroll but must not overlap, clip focused controls,
  or require horizontal scrolling.

## Accessibility And Focus Contract

- Search uses combobox semantics with `aria-autocomplete="list"`, `aria-controls`, `aria-expanded`, and
  `aria-activedescendant`.
- Results use `role="listbox"`; rows use `role="option"` and exact `aria-selected`. DOM option IDs are local render
  indices, not `resultId` or `appId`.
- One `role="status"`, `aria-live="polite"`, `aria-atomic="true"` region announces current result count, selection,
  lifecycle notices, successes, and errors. Raw thrown values are never inserted.
- Focus remains in the search input during result navigation, and the active option is scrolled to the nearest visible
  list position. New launcher events focus/select the input; settings events focus the settings heading; `关闭`
  relies on Rust hide and performs no local focus transfer; settings operations restore the initiating control only
  while they still own the current epoch.
- Every settings input has a persistent label. Every command is reachable by Tab and has a visible focus state.
- The generic application mark is decorative. No icon-only control is required by this minimal design; if one is later
  approved, it must have an accessible name and tooltip.
- Narrator must announce the combobox, result count, active option title/subtitle, settings view, and errors. The final
  Narrator smoke waits for Task 6 real event integration.

## Fixed Error Presentation

Task 7 recognizes only the fixed `CommandError.code` allowlist and uses local text. It never displays a thrown object,
stack, raw Tauri error, path, hotkey parser detail, or backend system message.

| Code | UI text |
|---|---|
| `staleRequest`, `unknownResult` | `搜索结果已过期，请重新搜索。` |
| `applicationEntryUnavailable` | `应用入口不可用，请重新扫描。` |
| `settingsFailed` | `设置未能确认完成；若快捷键或开机启动行为异常，请重启 UiPilot 后检查设置。` |
| `validationFailed` | `验证数据操作失败。` |
| `windowFailed` | `窗口操作失败。` |
| `scanFailed`, `scanWorkerFailed` | `重新扫描失败。` |
| `mainThreadDispatchFailed`, `exportFailed`, `exportWorkerFailed` | `导出失败。` |
| `invalidCaller` or unknown shape/code | `操作不可用，请重试。` |

Lifecycle notice text is likewise local and fixed:

- `settingsFailed`: `快捷键或开机启动设置可能未完全应用，请重启 UiPilot 后检查设置。`
- `validationFailed`: `本地验证数据操作失败。`

All user/backend strings render only as escaped React text children or plain text-valued props. Task 7 never uses
`dangerouslySetInnerHTML`, raw HTML parsing, or an AntD prop that interprets backend text as markup.

## Vitest Contract

Vitest uses the existing jsdom dependency, React 19 `createRoot`/`act`, and an injected fake client. It adds no Testing
Library, browser runner, snapshot package, or component-test dependency. No test starts Tauri, invokes a real system
plugin, changes autostart, registers a shortcut, opens a native dialog, or hides a real window.

The plan must turn these contracts into RED/GREEN groups:

1. Store-identity RED calls `getSnapshot` repeatedly and gets the same reference while unchanged; one real mutation gets
   one new reference and one notification; stale/no-op input keeps the same reference and sends zero notifications;
   `getSnapshot`/`subscribe` references remain stable; unsubscribe and unmount cleanup are idempotent. The React root
   then mounts through `ConfigProvider` and AntD `App`; the real input ref and native listeners become ready before event
   registration resolves, and listener registration resolves before the first `load_settings` call. Mount, ref,
   native-listener, or event-listener failure produces zero load calls. A shown event fired while load is pending is
   received and rendered.
2. Launcher/settings events validate exact payload shape, replace invocation, reset sequence/request/results while
   preserving/selecting only committed query text, switch one WebView, and apply the frozen focus target. A launcher
   target with a preserved non-empty query makes exactly one new-invocation search at sequence `1`; empty makes zero
   calls; the prior response is rejected. Nullable lifecycle/activation notice remains first while auto-results hydrate.
3. Every search start clears ordinary status while retaining an independent shown-level notice. Empty input makes zero
   search calls and invalidates a late response. Non-empty input sends exact command name and
   `{ query, invocationId, querySequence }`. A current matching `null` releases pending without replacing
   request/results/status, leaving ordinary status empty; stale response/error/null is fully zero-effect and cannot
   clear a newer pending search.
4. Pure classified-core RED replays launcher and settings rows for: no-tail boundary commit; boundary followed by a
   same-value tail; ordinary input before boundary; cancel `deleteContentBackward` before end; duplicate/no-start/
   wrong-target/unbound end; script-mutated DOM sentinel; no-tail shown/unbind/replacement/retirement discard; a later
   same-value ordinary no-op; and a different later ordinary edit. A matching boundary commits only `lastTrustedDraft`
   once. Boundary-tail and later same-value rows publish/call zero times; cancel-to-original may publish only the visible
   draft restoration while making zero search/settings call. A different value performs one normal edit and applicable
   search/settings mutation. No-owner, stale, retired, and replaced-control records stay zero-effect. Native-adapter RED
   proves only trusted same-target start/input can supply text, while the sole untrusted-event exception is a correlated
   zero-payload boundary from a same-target end with an active trusted-start binding. The post-GREEN WebView2 gate repeats
   these exact natural and adversarial rows. For launcher and settings, direct
   `keyDown(..., true)` calls preserve the exact snapshot and make zero client calls; a subsequent trusted non-composing
   `deleteContentBackward` record then takes the ordinary path exactly once, including one applicable launcher search or
   one settings-local commit with zero Rust calls.
5. Enter sends exactly `{ requestId, resultId }`, never `appId` or action data, and a second Enter while pending makes
   no second call.
6. Successful execution makes no hide/window call; failure preserves UI and announces fixed text; activation-refused
   success creates the one-shot next-show notice. A regression fires a newer launcher event before the execute Promise
   resolves and proves the late trusted refusal is retained for the following launcher event without mutating the
   already shown view.
7. Non-composing launcher Escape, settings Escape, and the settings `关闭` control call only `hide_launcher`, prevent
   native/local switching, and share one pending owner: while any same-epoch hide is pending, every trigger makes zero
   second calls. The control's visible text and accessible name are exactly `关闭`. Current hide rejection releases
   pending while preserving view/query/focus and announcing fixed text; stale completion is visually ignored and
   cannot release newer ownership. A source assertion rejects Tauri window imports, direct `.hide()`, and unapproved
   command/event names.
8. A protocol/client test records all eight command names, exact outer argument objects, outcome unions, nullable event
   notice, and especially `{ settings: update }`.
9. Settings renders every current application in order through AntD Form/Input/Checkbox/Button primitives, including
   empty aliases and same-name/different-ID targets. It never renders IDs in text, attributes, element IDs, React keys,
   form names, or serialized DOM; editing one duplicate changes only that core-private ID's save vector. Removing or
   replacing a composing field retires its native listener/control generation before a fresh local key can be used.
10. Save includes every current app ID with exact alias vectors, omits empty research ID, never emits display/icon/path
    fields, preserves edits on failure, and reloads once only after success.
11. Rescan/reload, export cancel/exported/error, and inline clear confirm/cancel/error have exact call counts, busy
    disabling, status text, and focus restoration.
12. DOM assertions cover the controlled AntD Input's exact combobox/listbox/option/status semantics, persistent labels,
    visible-state attributes, and keyboard-only reachability. Long Chinese/Latin text and markup-like titles render as
    literal text; a source assertion rejects `dangerouslySetInnerHTML`.
13. Source/component assertions admit only the approved AntD primitives, reject `AutoComplete`, `Select`, `Card`,
    `Modal`, `Popconfirm`, notification/message globals and direct `@ant-design/icons` imports, and prove that React/AntD
    view modules receive no Tauri client or command/event API.
14. Destroying the React root/core removes native DOM listeners and calls the Tauri event unlisten callback once.
15. A new shown event while save, rescan, post-operation load, export, clear, or hide is pending invalidates every old
    visible continuation: no stale form replacement, follow-up call, status, or focus restore. The single operation
    token still prevents overlap, its exact completion releases busy state without touching newer ownership, stale
    save/rescan marks only internal reload-needed state, and the next current settings entry or explicit reload loads
    once. Until that load succeeds, editable fields/Save/Rescan stay disabled. A retained-alias regression makes an
    absent app current during rescan, fires shown before rescan settles, proves the old form cannot Save, then reloads
    the full target/alias before any update is allowed. The startup readiness load remains the sole tested hydration
    exception.

Layout at 100/150/200% and Narrator behavior cannot be proven by jsdom. After R3 corrective Code Go, the production
WebView2 gate, and real wiring, run a
separate Windows WebView smoke for zoom, long text, focus restoration, Narrator search/execute, launcher target,
settings target, lifecycle notices, and Arrow navigation keeping the active descendant visible at each zoom level. It
is not evidence for the unresolved runtime ACL probe.

## Security Boundary

- The WebView listens only to `launcher://shown` and invokes only the eight existing commands.
- Search/execute send only query ownership IDs and result IDs. Settings IDs never enter action flow.
- No Tauri window, shortcut, autostart, tray, shell, process, filesystem, HTTP, or dialog API is imported by Task 7.
- No raw command error, query, result title, alias, app ID, or settings value is logged or persisted by the frontend.
- No remote image, script, page, font, or HTML is loaded. Optional icon strings are not used as URLs.
- AntD/React receives no injected Tauri client, and no view/component imports `@tauri-apps/api`.
- Only a real trusted same-target start may create ownership, and only trusted same-target input may supply text. The
  sole untrusted-event exception is a same-target `compositionend` converted to a zero-payload boundary while that exact
  binding has an active trusted-start session. It atomically clears binding ownership and carries/reads no data or DOM
  value. Every other untrusted raw event is zero-effect.
- Script can only prematurely finalize `lastTrustedDraft` already supplied by trusted composing input during that active
  session. It cannot inject text, create ownership, execute, hide, or invoke Tauri. The exception depends on the existing
  local-only page, unchanged CSP, `withGlobalTauri: false`, and no plugin/remote-content policy; changing any prerequisite
  stops for written security design review.
- The exact Task 7 product allowlist is the same ten paths frozen in Ownership And File Boundary. Of those ten, only
  `package.json`, `package-lock.json`, and `tsconfig.json` are non-source deltas; the other seven are the exact frontend
  source/style/test paths listed there. Capability, permissions, CSP, `vite.config.ts`, `index.html`, Tauri config,
  security probe/config/scripts, build scripts, Cargo/lockfiles, and Rust remain byte-identical.

If Task 7 needs a new command, event, permission, DTO field, icon URL grammar, unreviewed dependency, CSP/config change,
or Rust trust-input change, work stops for written design/security review. It is not folded into implementation as a
convenience.

## Task 6 Ownership And Integration Boundary

The following split governed Phase A and remains the ownership record. Task 6 is now integrated at `a8626e72`; that does
not waive the R3 design/code gates or transfer any Rust file to Task 7:

```text
Allowed before Task 6 Code Go
- package.json/package-lock.json exact reviewed dependency delta
- tsconfig.json exact JSX compiler setting
- src/protocol.ts DTO/event/client types
- src/launcher-core.ts injected-client ownership core
- src/native-input.ts real-event classifier boundary
- src/launcher-view.tsx thin React/AntD view
- src/launcher.test.tsx pure-core, native-boundary, and thin-view tests
- src/styles.css reviewed AntD-token/launcher/settings styles

Originally gated by Task 6 Code Go/local integration; now additionally gated by R3 Code/Security Go and its production gate
- src/main.ts real @tauri-apps/api listen/invoke adapter and startup
- any claim that load_settings marks frontend ready or drains a real pending target
- real launcher://shown launcher/settings/notice behavior
- final integrated production build/bundle/performance evidence against Task 6
- Windows WebView focus/zoom/Narrator/E2E smoke
- final main integration
```

The Phase-A commits remain frontend-only and independently reviewed. Real wiring and final integration remain bound to
the exact integrated Task 6 commit plus R3 Code/Security Go, never merely to a plan or implementation worktree.

## R1/R2 Results And R3 Production Hard Gate

R1 Pass A is a completed No-Go input, not a production pass. R2 corrective RED/GREEN passed in isolation, but its
post-GREEN gate repeatedly observed natural commit with no trusted non-composing tail on both launcher and settings.
R2 Design/Plan/Security Go are revoked, its diff was restored, and Steps 6/7 were not run. No further pre-TDD diagnostic
is required by this R3 revision.

After written R3 Design/Plan/Security Go and corrective GREEN, a temporary uncommitted harness must rerun launcher and
settings no-tail natural commit, end plus same-value tail, ordinary input before end, cancel delete plus end, lifecycle
discard, different later edit, and same-value idempotence rows through the real production adapter/core. It records only
versions, event kind/class, `isTrusted`, `inputType`,
`InputEvent.isComposing`, category, record kind, fixed counts, and fixed PASS/FAIL booleans; never data/text/value/query/
alias/ID/timestamps/backend payload. Required results are:

1. trusted composing input updates only draft; an active same-target end emits one zero-payload boundary and commits the
   stored draft once, including when the end is untrusted;
2. boundary plus same-value tail and ordinary input before boundary each produce one logical commit; exact-value follow-up
   input is a no-op, while a different later value is one normal edit/call;
3. cancel's trusted non-composing delete commits once and makes the later end zero-effect;
4. duplicate/no-start/wrong-target/unbound/stale/retired/replaced-control ends remain zero-effect, and a script DOM
   sentinel proves boundary never reads end data or current DOM value; and
5. shown/unbind/replacement/retirement before a boundary or ordinary input discards draft and restores committed value.

Any mismatch stops without commit and returns to written design/security review. The harness must be removed and the
worktree restored to its exact corrective dirty set before commit. This is not runtime positive-probe evidence;
`ReleaseSecurityBlocked / SEC-RUNTIME-PROBE-001` remains.

## Task 9 Local Code And Release/QA Split

Status: **DesignGo / PlanReviewRequired / SecurityReviewRequired / LocalTaskCodeGoPending / ReleaseNoGo**. Task 8
Code/Security Go remains bound to clean product HEAD
`16018e56486bcd4efcd1a2c81798ebc9223025e7`. This revision changes no Task 7 product code, command, permission, CSP,
dependency, or release status.

The custom Job cleanup preflight, historical performance runner, temporary measurement wrapper, CDP collectors/probes,
and every associated path, broker, identity, stage-counter, query-seed, and cleanup diagnostic are permanently
non-executable failed evidence. The final identity diagnostic failed before producing identity fields, so it supports no
executable-identity conclusion and does not justify changing full-path authentication, native Job structures, or product
code. No later design may resume or extend those scripts by treating this section as latent authorization.

Local Task 7 code review is separate from release/QA acceptance. A local `TaskCodeGo` request may rely only on the
already-approved Task 8 product HEAD and R3 Code/Security evidence, fresh frontend focused/full tests and Vite build,
inherited Rust and static security checks, exact ten-file product scope/trust inventory, and the approved initial
JavaScript/CSS bundle thresholds. It may not claim a release executable, runtime performance, cleanup certification,
WebView smoke, or release readiness.

The following remain unresolved release/QA blockers: 30 cold and 205 warm performance samples; certified process-tree
cleanup; final 100/150/200% zoom, Narrator, forced-colors, long-text, focus/navigation, and zero-network smoke; the
runtime-positive probe; installer, signing, trial, and release. `ReleaseSecurityBlocked / SEC-RUNTIME-PROBE-001` remains.

### Historical cleanup ownership attempts (permanently non-executable)

The formal zero-sample preflight under plan `4e9af0789974577521d444043890f2ecbaf59eeb` failed before query seed or
any performance sample with exact `Cleanup=FAIL category=preflight count=1` and
`child postdates authenticated parent exit cutoff`. The measurement build and no-product fixtures passed, but the
retained-tree cleanup inferred descendant ownership only after process exit. WebView2 can create a real child after an
authenticated parent's recorded exit cutoff, so that inference cannot prove the child is owned. Later physical zero
process/listening-port/TEMP/environment/worktree residue does not overwrite the failure. No result from that run is a
performance or release pass, and the timestamp model, runner, standalone diagnostics, query seed, 30/205 workflow, and
runtime release/QA claims remain closed. The separate static local trust checkpoint does not inherit this failed
infrastructure.

### Historical Job candidate (permanently non-executable)

The final rejected Task 9 candidate attempted to replace post-exit PID/parent/timestamp inference with
kernel-established job membership:

1. Create one unnamed Job Object for each primary measurement lifetime. Before any process is assigned, set only
   `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`; never set `BREAKAWAY_OK`, `SILENT_BREAKAWAY_OK`, UI limits, or a named/shared
   job. Windows associates child processes with their parent's immediate job by default, including nested job chains.
2. Create the exact release executable with `CreateProcessW` and `CREATE_SUSPENDED`, retain its native process and thread
   handles, and call `AssignProcessToJobObject` before the first `ResumeThread`. The product therefore executes zero
   instructions before membership is established. If create, limit setup, assignment, membership verification, or
   resume fails, terminate through the assigned Job or the unassigned retained process handle, require the native
   terminate and wait results, and attempt every thread/process/Job handle close independently. Preserve the operation
   error; if any cleanup also fails, report the operation error plus every cleanup failure. Cleanup success rethrows the
   original operation error unchanged. An empty catch or discarded terminate/wait/close result is forbidden.
3. Create every real no-argument secondary through the same suspended-create/assign/resume helper and the same primary
   Job Object. A secondary is never launched first and adopted later. It must exit `0` through its retained process
   handle; its handle remains retained until the owning job record is disposed.
4. Do not request `CREATE_BREAKAWAY_FROM_JOB`. A WebView2 descendant created by any process in the job remains a member
   unless an ancestor job permits breakaway; this Task 9 job does not. If the host's existing job hierarchy prevents
   assignment, or WebView2 cannot run inside the resulting nested hierarchy, the suspended process is never resumed or
   the real zero-sample preflight fails closed. The runner must not add a breakaway flag to make the environment pass.
5. Port and foreground checks may observe a PID only long enough to open a verification process handle. Before using
   that observation, call `IsProcessInJob(handle, job)` and require a live handle. Never retain a bare PID as ownership,
   never kill by PID, and never infer membership from executable name, parent PID, command line, or timestamp.
6. Cleanup calls `TerminateJobObject` once through the retained job handle, then polls
   `QueryInformationJobObject(JobObjectBasicAccountingInformation)` until `ActiveProcesses == 0` within the fixed
   deadline. It waits every retained primary/secondary process handle, verifies zero exact executable and zero
   `Listen`-state port residue, closes each retained process handle exactly once, and closes the Job handle last.
   `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is a crash/failure fallback, not a reason to skip explicit termination and
   accounting. Any failure is retained as `Cleanup=FAIL`; later zero residue cannot turn it into PASS.
7. Top-level recovery attempts browser arguments, `CARGO_TARGET_DIR`, `CARGO_INCREMENTAL`, and `CARGO_BUILD_JOBS`
   restoration independently before fixture files, TEMP target, and final residue checks. One environment restoration
   failure must not skip any later cleanup. Primary launch failure, browser-environment restoration failure, Job cleanup
   failure, and top-level cleanup failures retain their separate fixed labels in the final error. A browser-environment
   restoration failure is always added under `browser arguments restore`, including when it is the sole failure.

This ordering follows the Windows contracts for suspended creation, job assignment, automatic child membership,
nested jobs, and kill-on-close: [CreateProcess flags](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags),
[AssignProcessToJobObject](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-assignprocesstojobobject),
[Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects), and
[Nested Jobs](https://learn.microsoft.com/en-us/windows/win32/procthread/nested-jobs). Completion-port messages are not
an ownership source because their delivery is not guaranteed; active-process accounting and job membership are the
authoritative checks.

### Rejected alternatives

- **Post-exit parent PID/creation-time inference:** rejected by the formal preflight. No tolerance window, cutoff
  relaxation, or reordered rescan can remove the ownership race.
- **Start normally, then assign to a Job:** rejected because the primary may create WebView2 descendants before
  assignment.
- **Completion-port notification inventory:** rejected as the sole owner list because notification delivery is not
  guaranteed. It may not authorize termination.
- **Bare PID, `Stop-Process`, executable-name, command-line, or final-residue ownership:** rejected because PID reuse and
  unrelated processes cannot be ruled out at the operation boundary.
- **Breakaway, Rust/Win32 product bridge, new command, service, helper binary, dependency, or permission:** rejected as
  unnecessary scope expansion. The native interop exists only in the temporary PowerShell runner's in-memory `Add-Type`.

### Historical evidence contract (permanently non-executable)

The rejected checkpoint required the following sequence. It is retained only to explain the failed evidence and must not
be run, repaired, or used to reopen Step 3:

1. A no-product positive fixture creates a suspended PowerShell process, assigns it before resume, lets it create a real
   child, proves Job accounting observed at least two processes, terminates only the Job, reaches active-process zero,
   and observes every retained handle signaled.
2. A no-product assignment-failure fixture passes an invalid/closed Job handle. The process remains suspended, its fixed
   sentinel file is never created, the retained process handle is terminated/closed, and no child or residue exists.
3. A no-product wrong-job fixture assigns a suspended process to Job A, proves `IsProcessInJob` against Job B is false,
   and proves Job B cannot authorize or terminate that process. Job A alone performs cleanup.
4. One real zero-sample cleanup preflight starts the authenticated measurement executable by suspended assignment,
   proves every 9227 listener and foreground process is a live member of that Job, requires Job accounting to show the
   primary plus at least one WebView2 process, raises the fixed local sentinel, terminates the Job, reaches active zero,
   and restores exact-executable/Listen-port/TEMP/environment/worktree state.

Fixtures authenticate structure sizes, constants, one `CreateProcessW`, one `AssignProcessToJobObject`, one production
launch helper, no breakaway token, retained-handle close counts, and fail-closed cleanup. They log only fixed fixture
names, counts, Win32 error codes, and PASS/FAIL; never command payload, query, UI text/value, ID, path, or timing. Any
assignment, membership, nested-job, WebView2 startup, accounting, cleanup, or residue mismatch is Design/Plan/Security
No-Go. A passing real cleanup preflight returns for separate written approval; it does not automatically reopen 30/205.
Before the native fixtures, one in-memory failure-preservation block must prove both browser-only and
launch-plus-browser-plus-Job-cleanup outcomes. The former retains `browser arguments restore`; the latter retains the
launch error plus both `browser arguments restore` and `primary Job cleanup`. Source checks must authenticate the
unconditional non-null browser labeling branch, reject a launch-and-browser conjunction, and reject empty catches and discarded
`TerminateJobObject`, `TerminateProcess`, or `WaitForSingleObject` results in every suspended-launch failure path.

## Local Bundle Gate And Unresolved Release Responsiveness

AntD's `48,735,523`-byte unpacked package size is not bundle evidence. Using the production-mode Vite build on the
same reviewed lockfile, sum every initial HTML-referenced local asset and record each file plus totals. Deterministic
gzip uses level 9 over the exact emitted bytes. Proposed Task 7 Go thresholds are:

- initial JavaScript: at most `900 KiB` raw and `300 KiB` gzip total;
- emitted CSS: at most `120 KiB` raw and `30 KiB` gzip total;
- zero remote runtime asset/font requests, dynamic remote chunks, source maps in release output, or security-probe HTML
  and related probe assets;
- cold local WebView `performance.timeOrigin` to mounted view, bound native listeners, and registered
  `launcher://shown` listener: P95 at most `750 ms` across 30 clean process starts on the agreed Windows reference host;
- warm `launcher://shown` callback entry to focused, enabled input and the next painted frame: P95 at most `100 ms`
  across 100 events after 5 warmups, for both empty and preserved-query launcher targets. Backend search time is reported
  separately and remains subject to the frozen search P95 requirement.

The initial JS/CSS size and emitted-asset checks remain part of the local code gate because they operate on the ordinary
Vite build without running product processes. The Windows/WebView2 responsiveness thresholds remain frozen release/QA
acceptance criteria, but no custom Task 9 measurement seam or runner is executable. Exceeding a bundle threshold,
losing tree shaking, or requiring a Vite React plugin returns to written design review; do not weaken the threshold or
hide cost by lazy-loading the first launcher view.

## Design Gate

This design requires written Design Go before an implementation plan is written. Plan Go is separately required before
creating a Task 7 implementation branch/worktree or changing any frontend file.

Task 7 real wiring and E2E additionally require Task 6 Code Go, approved local integration, and the WebView2 metadata
hard gate. Passing pure-core/jsdom tests cannot waive those dependencies. `ReleaseSecurityBlocked /
SEC-RUNTIME-PROBE-001` remains throughout; Task 7 does not run or repair the runtime positive probe and does not
authorize merge, push, signing, trial, or release.
