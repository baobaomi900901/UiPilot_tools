# Host 0.10.0 — main-result actions and an offline snippet tool

2026-10-25. Reviewed implementation contract: independent Spec PASS on 2026-10-25; user-authorized autonomous development. Baseline: accepted Host 0.9.0, HEAD `4a3b994`, in `C:/Users/moby/.codex/worktrees/local-plugin-foundation/UiPilot_tools`. This specification is frozen before implementation. Historical M2-B candidate/review/rebase/readiness documents were read as evidence; their 0.6.0 version, absent Panel/Window RPC and emit-before-watchdog descriptions are obsolete.

## Outcome and choices

A main-result row can offer named host-copy and Runtime business actions. Enter executes an explicitly declared default; a visible footer “动作 Ctrl+K”, right-click, and keyboard menu expose alternatives. A fresh `onAction` may save plugin-private data and return a replacement result snapshot. The useful local example transforms text, explicitly saves the chosen text, and explicitly reads the latest saved snippet.

Recommended: bounded copy + distinct `onAction`, reusing the existing Runtime, serial scheduler, request-bound APIs and completion ticket. Host-copy-only alternatives are smaller but cannot save-and-refresh. Encoding actions into `onCommand` input or exposing the private host ResultAction enum would confuse command semantics or expand authority, so neither is adopted.

Scope is Windows, mainResult only. No online install/publishing, arbitrary files, Shell, paste, URLs, extra network authority, new Panel/Window APIs, standard UI framework or per-command shortcuts. Shortcuts remain the separately reviewed next vertical; this release does not claim the entire local foundation is complete.

**Copy keeps the launcher open in this increment.** It displays “已复制” and disables the consumed source snapshot's actions. Current main hide has no atomic owner-check/native-hide operation against async query publication; main-thread dispatch alone cannot close that race. Do not build a new native hide controller for this release. Undeclared old plugins retain their historical copy/hide behavior.

## Current integration evidence

| Current seam at baseline | Required change |
| --- | --- |
| `commands.rs:3742` public publisher drops plugin_result_id and publishes optional copy text | Retain business result/action IDs, original input and full private owner; publish only opaque display handles |
| `result_registry.rs:101,130,286,376,421`: execution ticket, one action per row, registry-locked publication authorization | Add bounded action bundles, snapshot claim and origin lease; no earlier-lock acquisition inside registry callbacks |
| `scheduler.rs:44,327,440,529,550`: Command/PanelAction/WindowAction, idle-only business admission, physical occupancy, completion ticket, retirement | Add ResultAction owner to every relevant match, not a second lane; use retirement, never cancel() to free possibly running JS |
| `public_plugins.rs:786–1049`: pending content registration/settlement, watchdog starts before lookup/emit, tagged dispatch | Add separate result-response pending kind; reuse transport/watchdog and exact settlement; result replies are not content data |
| `runtime.rs:210–288`: shared busy/fresh facade, bounded content snapshot, expire before completion IPC | Add declared onAction readiness/tag and bounded result snapshot branch |
| `activation.rs:3586,4120,4333`: confirmed copy authority, action admission, manager completion claim | Add full result-owner authority and atomic source admission/publication; boolean can_copy_text followed by unprotected copy is insufficient |
| `plugins.rs:255`: existing admission-write invalidate_plugin_results | Preserve; add narrow admission-read composition on this same manager, not a new lock |
| `commands.rs:5235,5652`: main/find execute and unconditional main clear/hide | Keep old endpoint; new main-only endpoint has no native hide |
| `launcher-core.ts:1927,2145,3304`: debounced input search and execute token/epoch/invocation | Immediate action-origin retirement notification on edit; capture source query sequence/value and handle before await |
| `launcher-view.tsx:1194`: favorite-only Dropdown | Shared menu presentation independent of favorite availability; discoverable footer and keyboard focus integration |

Ctrl+K has no competing launcher handler in the current source. Existing suffix-control, IME, management inventory revision and focus owners remain in use. Public result rows remain non-favoritable; reusable command discovery entries retain their existing favorite path.

## Manifest, compatibility and public SDK

Add optional boolean `runtime.resultActions`: absent/false preserves legacy behavior; true requires `minimumHostVersion >= "0.10.0"`, exactly Windows supportedPlatforms, and every normalized command outputMode mainResult. Both legacy `command` and new `commands` forms can opt in, with live or submit activation. Null, wrong types, extra fields and incompatible output/platform/minimum reject. This follows current Panel/Window boolean opt-in conventions. It is a capability declaration, not a permission.

Opt-in Runtime readiness requires callable exports `onCommand` and `onAction`, even for a copy-only action package. No handler inference from a returned row, no silent fallback. A legacy module's extra onAction export grants nothing. CLI remains static and does not evaluate JavaScript.

Keep apiVersion/schemaVersion 1, state schema 2, CLI report schema 1 and the CLI package's independent version. Synchronize actual Host package/Cargo/Tauri versions, Host::current and CLI production target to 0.10.0; do not raise historical example minima. Update Rust Manifest DTO/validator, generated schema, CLI parity fixtures, SDK types, contract and developer tutorial together. Correct touched stale feature-summary prose rather than treating it as current authority.

`onCommand` remains unchanged: legacy command invocation omits commandId entirely; commands-form invocation supplies it. Legacy result parsing, optional null behavior and limits remain unchanged for undeclared packages.

New result form, available only with the true capability:

```ts
type PluginResultAction =
  | { id: string; title: string; type: 'copyText'; text: string }
  | { id: string; title: string; type: 'runtime'; payload: JsonValue }

type PluginActionResult = {
  id: string
  title: string
  subtitle?: string | null
  detail?: string | null
  actions: PluginResultAction[]
  defaultActionId?: string
}
type MainResultResponse = {
  requestId: string
  results: (PluginResult | PluginActionResult)[]
}
type PluginActionInvocation = {
  apiVersion: 1
  requestId: string
  commandId: string
  resultId: string
  actionId: string
  payload: JsonValue
  input: string
  context: { platform: 'windows'; theme: 'dark' | 'light'; invokedAt: string }
}
type PluginActionHandler =
  (invocation: Readonly<PluginActionInvocation>, api: Readonly<UiPilotPluginApiV1>)
    => Promise<MainResultResponse>
```

The new onAction commandId is always the saved normalized ID, including default for opted-in legacy command. resultId/actionId are plugin business IDs. Payload and input are immutable Host-held snapshots, not current input text. Context theme/time are sampled at dispatch. requestId is a new scheduler ID. Invocation/API are fresh and deeply frozen; APIs from completed onCommand remain expired.

Exact validation:

- New rows have 0–8 actions. Empty means informational. Missing defaultActionId means no default even with one action; a present value must name exactly one action in that row. Null is invalid. Enter on a no-default row opens its action menu if nonempty; it does not silently choose the first action.
- actions and legacy defaultAction cannot coexist, even when defaultAction is null. defaultActionId without actions rejects. Opted-in responses may mix legacy rows and new rows; all normalize to the same snapshot policy.
- Action IDs match `^[a-z][a-z0-9-]{0,31}$`, unique within their row, case-sensitive. Business ID constructor remains valid; use Maps/own-key comparisons. New result IDs: nonempty, <=128 UTF-8 bytes, no C0/C1 controls. All result IDs in an opted-in response must be unique; old undeclared parsing is not narrowed.
- Titles: 1–80 Unicode scalars, at least one non-whitespace character, no C0/C1 controls; no coercion. Copy text: <=16 KiB UTF-8, NUL forbidden, empty allowed. Runtime payload is required and may be null.
- Keep whole result response <=64 KiB compact UTF-8, <=20 rows, current row title/subtitle/detail limits. For opted-in responses reuse content_action bounded JSON limits (depth <=32, <=4096 values, root depth 0/root counted), applied to the complete response, rather than inventing a second payload budget. Legacy results from undeclared packages keep existing semantics.
- At JS snapshot and Host independently enforce supported finite JSON; reject cycles, accessors, symbols, sparse arrays, undefined/functions/BigInt, nonplain objects and dangerous record keys __proto__/prototype/constructor. Inspect descriptors/captured intrinsics without invoking getters or toJSON. These key restrictions do not ban business ID string values.
- Envelopes, rows and action variants have exact keys. Unknown fields/type, missing fields, bad defaults, duplicates or oversized data reject the whole response. No dropping dangerous actions, truncation or stringified-response fallback. After framework conversion to Value, do not claim duplicate raw JSON keys can still be detected.
- Any copy action requires current declared/granted clipboard.write at publication and actual effect admission. Storage needs no new permission. Other Runtime effects retain existing permission, network and credential checks.

onAction returns only the bounded mainResult response with its fresh scheduler requestId. Success replaces the entire source snapshot, including empty results, with fresh Host handles. No data envelope, patch, retain-old option, recipient, hide/open request or command retargeting is accepted. Business validation returns ordinary informational results; thrown/rejected handlers use fixed failure.

## Private IPC and identity

Use a **new** main-only private command, `execute_result_action`, with `{request:{requestId,resultId,actionId}}`. All three are opaque Host-generated strings and required. Validate the nested DTO's exact fields and types before lookup; no plugin ID, command ID, input, payload, text, generation or retry token is accepted. The actual WebView label must be main before state access. Find, shell, Panel, Window content and Runtime callers fail. Register AppManifest/build.rs, lib.rs, generated ACL and the narrowly scoped main capability together. Do not add the command to plugin-content capabilities.

Keep execute_result({requestId,resultId}) and its old outcomes unchanged. New opted-in rows use execute_result_action for default and alternative actions; if execute_result is presented an opted-in action row it fails closed instead of bypassing its snapshot claim. This avoids changing old flat IPC argument compatibility or Find parsing.

New sanitized row metadata contains `actions:[{actionId,title}]` and optional `defaultActionId`, both action identifiers being opaque handles. hasDefaultAction agrees with the optional opaque default. No business IDs, type, text, payload or authority are serialized. The frontend validator rejects inconsistent/malformed metadata; it never derives authority from labels.

Return a discriminated `ResultActionOutcome`:
- `{kind:"copied"}`
- `{kind:"resultsUpdated",response:SearchResponse}`
- `{kind:"failed",code:ResultActionErrorCode,sourceConsumed:boolean}`

Normal caller/DTO rejection can use existing fixed CommandError. Execution failures use the typed outcome so the UI never guesses whether admission occurred. The SearchResponse requestId is a fresh **registry** request handle, distinct from the plugin's scheduler requestId. Its mainResultCommand is generated from saved command/input, never response-supplied.

Host-private origin stores Main scope, scope generation, invocation ID, query sequence, result-set generation/registry request handle, row handle, action handle, plugin ID, command ID, Runtime generation, activation ID, admission epoch, original argument and business result/action/payload. Retain the existing ExecutionTicket as one component; do not pretend it already contains all fields. Identical IDs in sibling commands or replacement snapshots never alias.

## One snapshot claim and origin lease

Retain a single snapshot-wide state for opted-in results: Available -> InFlight -> Consumed. Both copy and Runtime actions claim it, including normalized legacy-form copies in an opted-in response. Busy or rejected admission leaves Available. Once admitted, every failure, uncertain send, timeout and successful copy consumes it; successful Runtime replacement creates a fresh Available snapshot.

This conservative historical rule is justified here: any Runtime mutation may invalidate sibling rows, and uncertain outcome cannot prove old action payloads remain appropriate. A per-action replay map plus sibling invalidation rules would add states without a useful workflow gain in this slice. It is not durable exactly-once delivery. New command execution, replacement or restart is new intent; plugins still own domain idempotency.

Duplicate during InFlight returns actionInProgress without another waiter/dispatch; duplicate afterward returns actionAlreadyExecuted. No client-generated nonce can restore a consumed snapshot. At most one current source claim and one pending result action exist for Main; history is not retained.

A tiny private `ResultActionLease` binds that snapshot's owner to an atomic live bit shared by the scheduler/pending record. It is not a content-window owner or new controller. Registry mutation sets it false **under the registry mutex** before dropping/replacing the source set. This applies to on_show/try_on_show, accepted begin_query, any successful publish/replacement, retire_result_set_if_current, hide_and_clear/try_hide_and_clear, matching application-query retirement, plugin epoch reserve/commit invalidation and invalidate_domain when they make this origin unusable. Failed/no-op operations do not revoke unrelated current owners. Reserving a plugin epoch makes the old origin unusable; cancellation does not resurrect a revoked action lease.

ResultAction context_status, with_current/effect admission, dispatch eligibility and terminal completion must check this bit, not just running.current. Thus no newly admitted Runtime API or result replacement survives a retired source, even before post-lock service cancellation runs. Scheduler completion records Cancelled/Expired rather than a successful ticket for an inactive lease. Existing issued-context distinction remains: a legitimate retired context yields ExpiredRequestError, forged context stays InvalidContext.

Registry code never calls manager/scheduler/service under its lock. Source retirement returns/drains exact bound result-action context identities for post-lock retirement/settlement; all Main query/show/hide/domain-management call sites wire this cleanup. The live bit closes the ordering gap. Cleanup calls retire_context through the existing network-authority transition, not scheduler.cancel(), then settles the one pending caller. It may be repeated safely. Keep bounded storage for only the current/pending source; do not grow a cancellation history.

Frontend sends a second narrow main-only command `retire_result_actions({request:{requestId}})` immediately on source input edit (before the existing 150ms search debounce), leaving mainResult, moving to Settings/Panel, or other local source-view departure. It only retires a matching current opted-in source; stale/duplicate request handles are harmless no-ops and cannot clear a replacement. Awaiting it is not required to update local UI. Native hide/show and actual Host query replacement independently revoke the lease. The guarantee begins when Host observes retirement, not before an IPC has arrived; frontend suppresses old completion immediately on local edit.

## Admission, locks and effects

Prepare bounded payloads, channels, pending record, opaque ID capacity and deadline readiness before live admission. Use the existing legacy PluginManager admission RwLock via one narrow bounded read-composition method; do not call its write invalidation helper recursively.

Nested order for a new action is: pending-result registration lock (only while installing) -> shared plugin admission read -> PublicPluginManager mutation -> network-authority transition when needed -> scheduler -> result registry. Pending registration is removed and its mutex released before any completion/cancellation callback reacquires owner/manager locks. There is no registry -> scheduler/manager path. Registry publication's existing authorize callback cannot acquire earlier locks.

Under this bounded joint section recheck exact source/lease/Available state, confirmed bundle, installed/enabled/nonfaulted/current generation + activation/admission, saved command membership/mainResult opt-in, grants, and idle Runtime (for Runtime actions). Atomically commit source claim, reserved request, physical slot, exact pending binding and timestamp/deadline. Any precommit failure leaves neither claim nor runnable request; all allocation/binding failures are resolved before exposure. Keep the caller's source data authoritative through this section, not merely a lookup done before spawning a worker.

Initial opted-in onCommand result publication must use this same shared admission-read and PublicPluginManager mutation protection, rechecking the current confirmed full plugin/command owner, generation, activation/admission identity, resultActions capability and every required copy grant through the final registry publication. The existing publisher callback `|| true` is not sufficient and must not be reused as authority proof. Prepare the bounded snapshot before acquiring these guards; hold the authorization protection until the registry atomically publishes or rejects the originating query token. This rule applies to the first result set as well as onAction replacements.

Shared scheduler admission rejects running, waiting, rebuilding/recovery and an unclaimed action completion ticket. It neither queues a result action nor replaces a command. ResultAction uses the existing 30-second submit timeout, even for a live source command. Query scheduling retains latest-wins waiting behavior. New command logically cancels ResultAction, but occupied JS remains until handler completion or original deadline/confirmed replacement.

Clipboard has no Runtime slot. Its source claim is the effect admission point. Hold existing public mutation protection through bounded clipboard execution so permission/lifecycle mutations cannot pass a check then revoke before effect admission; release registry/scheduler locks before clipboard I/O. Source retirement after admission can suppress feedback but cannot undo the admitted copy. No JS callback, await, native hide, network send, thread join or arbitrary plugin work occurs under registry/scheduler locks.

Public management preserves current order: perform/settle operation effects; release public mutation/authority/pending guards; call admission-write invalidate_plugin_results; release admission; cleanup. M4 remains mutation -> authority -> scheduler -> network registry; no authority acquisition from scheduler/data-gate callbacks, no secret authority revival from a result handle. Existing checked confirmed-bundle reopening, network budgets, redirect/delivery checks and terminal shutdown remain unchanged.

## Completion, cancellation and failures

Extend the existing shared scheduler completion ticket to ResultAction in every relevant dispatch/retirement/rebuild/fault path. Service pending keeps a result-specific origin and typed response channel; do not masquerade Main as ContentActionIdentity::Panel/Window. Reuse ContentActionError categories and valid_json helpers where useful, but parse result responses as results.

Normal completion physically retires the handler before service parsing: its API context is already expired. Success requires the saved matching terminal ticket, live source lease, current confirmed bundle/command/capability/grants, and exact origin compare-and-replace. Do not require an active facade or reopen it. Claim the completion ticket and publish the new set inside the same protected manager/authority -> scheduler -> registry composition, not a permission check followed by unlocked publication. Remove/discard the ticket on every terminal path. Replacement does not call begin_query and cannot steal a newer query.

Bootstrap separates handler throw (`failed:true`, no arbitrary text) from failed result snapshot (`failed:false`, null response). Expire facade and clear busy before the single complete_plugin_command IPC, as today. A valid authenticated completion with malformed nested response must settle invalid response, not linger until timeout. Caller/context that cannot authenticate a pending owner selects nothing; watchdog governs legitimate pending work. Completion transport failure does not emit a second completion.

The current watchdog already starts before window lookup/emit. For this action path ensure fallible observer readiness is obtained before committing admission, then arm its captured request/deadline before any transport. It survives removal of the UI pending record, uncertain emit and completion-IPC loss. Emit is attempted once. Uncertain send settles unavailable once but cannot release possibly running JS; retain original deadline and timeout/rebuild. Prove autonomous progress without another user event; do not add a second timer authority.

| Event | Authority/scheduling | Visible behavior |
| --- | --- | --- |
| Selection, scroll, menu open/close, theme | Keep source lease | Current action can replace original set despite selection change |
| Input edit, resubmit, sibling command, new query/view | Revoke source; retire exact request; retain physical slot | Suppress old result/error/status; latest query waits if same Runtime busy |
| Native hide/show, Settings/Panel departure | Revoke immediately at observed Host transition | No late UI/focus effect or reopened launcher |
| Real rename, settings/secret/grant change, disable/update/reload/uninstall/fault | Existing package retirement + lease/ticket revocation | No stale action authority or success |
| Favorite-only / exact no-op mutation | No business cancellation solely for that event | Actual source replacement can still suppress feedback |
| Handler completion | One terminal ticket, fresh result validation, compare-and-replace | Keep open; first-row selection; never autoexecute |
| Timeout / transport loss | Consume source; retire exact context; existing replacement/fault rules | Fixed current-owner error; read current saved data before deliberate retry |
| Shutdown | Terminal revocation | No late publication or authority reopening |

Fixed codes: actionBusy, actionInProgress, actionAlreadyExecuted, actionFailed, actionInvalidResponse, actionTimedOut, actionUnavailable, actionCancelled; reuse staleRequest, unknownResult, pluginPermissionDenied and clipboardWriteFailed meanings. sourceConsumed is true iff this source was already claimed, even on failure. Busy/pre-admission denial is false. Stale-source errors need not disclose another source's claim. Handler failure/invalid response/timeout follow existing runtime fault accounting; busy, replay, user cancellation and pre-admission rejection are not faults.

For an admitted current-owner failure keep text visible and all source actions disabled, with “操作未完成，请重新读取核对后再运行”. Successful copy shows “已复制” with all source actions disabled. Provide an explicit “重新运行” footer button which invokes the ordinary command submit path, never onAction or a retry queue. The sample's onCommand is read-only. Existing arbitrary plugins may perform effects in onCommand; this control is explicit user intent, not an automatic recovery promise.

A staleRequest or unknownResult outcome also closes the captured source menu and disables that captured source snapshot, even when sourceConsumed is false; show “结果已过期，请重新运行” with the explicit Run Again control. Apply this only while the captured source still owns the UI, never to replacement results. In particular, cancelling a plugin-epoch reservation does not revive its already-revoked action lease, so retained row text must not imply that its actions remain usable.

Timeout error feedback is allowed to reach its still-current captured UI owner even after Runtime replacement, without granting success-publication or API authority. If source UI changed, suppress it. Diagnostics log only fixed codes/stages; no payload/input/copy text/result detail, raw exceptions, secrets or paths.

## Launcher interaction and frontend ownership

The footer exposes “动作 Ctrl+K” when the selected row has actions, including rows without a default; disabled during local pending/consumed state. Right-click selects that same row and opens the same menu. Action items are separate from favorite actions with a separator when a row legitimately has both; result rows do not acquire favorite metadata.

Ctrl+K applies only to active launcher results, with no Alt/Shift, outside IME/composition and no handling in Settings/Panel/Find. Enter/click on a default row executes its declared opaque action; on a menu-only row it opens the menu. Arrow keys/Home/End navigate menu items, Enter selects, Escape closes menu before launcher escape, Tab closes/menu-focus advances through active suffix input -> footer Actions/Run Again when present -> Settings. Restore focus only if origin still current and user has not moved focus. Native right-click/menu selection must not also trigger row default.

Before await capture execution token, view epoch, invocationId, querySequence, query/control value, source registry requestId, plugin/command context and action handle. Pending cleanup, status/error and results installation require that captured owner still matches. Highlight changes alone do not invalidate it. Atomically install new request handle, mainResultCommand and rows only after source comparison; do not compare the newly returned handle to the old source handle, and do not call finishSearch/begin_query to apply an action replacement.

Preserve active suffix control identity/caret when the saved command identity/label is unchanged. Replacement selects first row by ordinary rules, with zero rows handled. Never steal focus back from another current control. Semantic tokens, stable single-line footer, 12px metadata, existing row geometry, Light/Dark, disabled/selected/focus distinction, accessible menu/button names and forced-colors behavior follow docs/ui-guidelines.md.

## Local example: Text Snippet

New independent `examples/public-plugins/com.uipilot.text-snippet/package/`, plugin version 1.0.0, Host minimum 0.10.0, Windows, runtime.resultActions true, only clipboard.write permission. Base package structure on demo-multi; borrow whole-record validation/revision handling from json-workbench without adding a window. Include a valid fixed icon.png so launcher discovery has an unmistakable identity.

Two submit/mainResult commands: stable IDs `transform` (defaultName text-snippet, required input) and `latest` (defaultName latest-snippet, no required input). Both handlers reject unknown/missing command IDs. onCommand never writes storage. Transform returns “原文” and “大写” rows with copy default and “保存为最新片段” Runtime action. Latest reads storage and returns one saved row with copy default plus “重新读取” Runtime action; no saved value returns an informational row plus a menu-only read action row. The same result/action business IDs may be reused across commands to exercise isolation.

Input/transformed text <=8192 UTF-8 bytes; reject overflow with fixed informational result and no save action. Storage key `snippet.latest`: exact record `{schemaVersion:1,revision:positive safe integer,text:string}`; absent record has revision 0. Unknown version, malformed record, non-safe revision and excessive text block all writes, never reset data. Transform reads current record, then save payload holds `{text,expectedRevision}`. onAction save independently validates payload, rereads/validates record, requires expectedRevision equal current and no increment overflow, then writes once with revision+1. A mismatch returns an informational conflict and a fresh read action, never overwrites. Storage write failure is not retried or represented as “not saved”.

onAction `read` has null payload and returns fresh saved/current results; save succeeds only after awaited storage.set and returns saved status plus copy/read actions with new handles. One Runtime serializes handlers, but explicit revision still protects stale snapshots and future content writers. No clipboard reads, user files, network, background tasks or destructive operations.

README/manual handoff clearly says choose the **package directory** for “选择开发目录”, or the produced **.uipilot-plugin file** for “选择插件包”; project root is not installable. Show exact slash commands and footer action location. No implementation/internal protocol details in the plugin-facing result text.

## Review and acceptance

Suggested three reviewable implementation batches: (1) backend+SDK/CLI/version and deterministic lifecycle/IPC tests, (2) launcher protocol/core/menu and focused UI ownership tests, (3) standalone example, package validation and separate manual delivery. Batch 1 must freeze wire/lease/claim contracts before batch 2; batch 3 may be prepared after SDK freeze. Independent reviewer checks the integrated result as well as each security boundary. No new persistent Host schema or dependency is expected.

Highest-risk changes are atomic scheduler+registry claim/completion, all source retirement entry points, request-bound API lease checks including network delivery, and async frontend ownership. This is a medium-size cross-layer increment, likely roughly 15–25 production/contract files plus focused tests and the new sample. Do not count existing M3 machinery as work to rebuild, or hide native copy-hide redesign inside it.

Focused automated acceptance (no exhaustive unrelated suite by default):
1. Manifest Rust/schema/CLI parity, missing-handler readiness, old command invocation omission and unchanged legacy parser; actual main-only command ACL/adapters reject forged fields/callers.
2. Identical result/action IDs across commands and sets; opaque handles cannot retarget input/payload/command; old execute_result cannot bypass opted-in claim.
3. Duplicate click/replay, busy without consume, precommit registration/watchdog failure without ghost request/claim; malformed bootstrap response versus handler failure classifications.
4. Actual barrier tests: registry retirement at each entry point; action lease checked by storage/settings/notifications/network admission and delivery; old API expires while physical handler retains slot; stale completed ticket cannot publish after permission/owner change.
5. No registry-to-manager lock inversion; shared admission read versus legacy reserve commit/cancel and public partial-error invalidation; poisoned/exhausted state fails closed.
6. Uncertain emit/completion loss produces autonomous timeout with no later user action, one settlement, no second dispatch and no replacement-generation damage.
7. Frontend local edit before debounce retires/suppresses old outcome; stale retire IPC does not revoke replacement; no-default menu; copy success/failure disables consumed actions; explicit Run Again never dispatches onAction.
8. Sample transformation, byte limits, bad storage schema, revision conflict/overflow and write failure without retry; package directory and archive static validation.

User-led Windows manual batches:
- Install the delivered sample, discover icon/commands, transform text; use footer, right-click and Ctrl+K. Copy shows 已复制, stays open, old actions disable; explicitly rerun and inspect clipboard in a scratch editor.
- Save transformed text; read through latest-snippet; close/reopen and restart Host to confirm persistence. No automatic save on transform. Repeated activation cannot double-save one snapshot.
- Invoke action then edit/switch/hide/reopen; no stale results/status/focus, no unexplained disabled replacement. Exercise Run Again/read after a reported uncertain action outcome.
- Light/Dark, long text, menu overflow, keyboard/Tab/Shift+Tab/Escape, IME and accessible labels.
- Short regressions: old demo-return/multi copy behavior, favorites, Notes, template Panel RPC, json-workbench Window RPC and pomodoro hidden timer using provided local packages.

User manual testing remains the native evidence gate. This design run performs no build/tests/install/native Host, clipboard/network operation or user-data access. Delivery later uses a new isolated 0.10.0 acceptance kit with source/hash manifest and preserves older kits. Independent review verified the initial-publication and stale-source amendments before implementation; no repeated approval of internal contracts is required by the existing autonomous-work authorization.
