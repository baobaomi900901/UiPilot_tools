# Panel Runtime actions and offline templates — approved design

Status: Host capability and offline template delivery implemented at source125e9d7, targeted checks passed, independent final Spec PASS / Quality Approved with zero remaining findings. Ready for user-operated acceptance; every new native/manual case is still pending. This is not completion of the full foundation.
Baseline: `8176c42f434ee18556b6c6c1acb3d6d1b42915bc`; no `.codegraph/` directory.
Delivery: Host 0.6.0 plus one runnable offline template plugin.

## Outcome and alternatives

A plugin panel submits bounded business requests to its own existing Runtime and receives JSON replies without reopening its launcher command. A text-template editor proves variable discovery, preview, validation, save, reopen and restart persistence. This differs from Notes, whose Runtime returns empty data and whose panel directly uses storage.

Recommended: explicit `panel.request` / `onPanelAction`, sharing Runtime, scheduler and authority checks. Encoding actions into `onCommand.input` conflates launcher ownership and JSON business calls. A separate worker or generic invoke bridge duplicates execution/admission authority. Neither alternative is needed.

Keep API v1 and existing command/panel/window behavior. Exclude launcher result-action menus, `onAction`, multi-command panels, Window RPC, arbitrary invoke/shell/files, direct WebView network, clipboard expansion, hotkeys and general UI kit. Result actions are later independent work, not a prerequisite. The older `m2b-actions-design-candidate.md` is not a requirement. Include the small user-observed install-label fix in delivery: visible “选择插件包” and “选择开发目录” beside existing icons, preserving handlers/accessibility and compact layout.

## Public contract

Add optional `panel.runtimeActions: boolean`. Absent/false means unchanged behavior; null/wrong types reject. True requires Windows, legacy `command` with `submit + panel`, granted `ui.panel`, and `minimumHostVersion >= 0.6.0`. Existing `commands` arrays still cannot declare panels. No new permission: all Runtime capabilities retain their current manifest/grant boundaries.

Opt-in additionally requires a function export `onPanelAction` during Runtime readiness, before candidate publication. Legacy modules need no new export. CLI checks declaration/platform/version but does not execute or promise to validate exports.

```ts
interface PluginPanelRequest { action: string; payload: JsonValue }
interface PluginPanelActionInvocation {
  apiVersion: 1
  requestId: string
  action: string
  payload: JsonValue
  context: { platform: 'windows'; theme: 'dark' | 'light'; invokedAt: string }
}
interface PluginPanelActionResponse { requestId: string; data: JsonValue }
type PluginPanelActionHandler = (
  invocation: Readonly<PluginPanelActionInvocation>,
  api: Readonly<UiPilotPluginApiV1>,
) => Promise<PluginPanelActionResponse>
// PluginRuntimeModule gains optional onPanelAction.
// UiPilotPluginPanelApiV1 gains optional request(input): Promise<JsonValue>.
```

`window.uipilotPluginPanel.request` exists only for opted-in panels. It resolves with frozen `data`, not the envelope. Each Runtime invocation/API is new and deeply frozen. Host creates request ID and captures the full live owner; no public plugin/command/session/generation selector exists. Internal epoch is canonical decimal. `onCommand`/`onUpdate` wire shapes stay unchanged; action replies do not trigger onUpdate or launcher results.

Action names: `^[a-z][a-z0-9.-]{0,63}$`, excluding `__proto__`, `prototype` and `constructor`. Payload is required, including explicit null; exact top-level fields only. JSON supports finite primitives, dense arrays and plain string-keyed records. Reject cycles, accessors, symbols, functions, undefined, BigInt, nonfinite values and record keys `__proto__`, `prototype` and `constructor`. Snapshot without getters/toJSON before IPC.

Limits: compact UTF-8 `{action,payload}` <=64 KiB; complete `{requestId,data}` reply <=64 KiB; nesting <=32 and <=4096 values in each direction. Bootstrap checks before IPC; Host independently checks. No truncation; overflow fails closed. Reply request ID must exactly match. Business validation errors are ordinary data, e.g. `{ok:false,code:'missing-variable',fields:[...]}`; unexpected exceptions expose neither plugin stack nor arbitrary native text.

| Error.name | Contract |
| --- | --- |
| `InvalidPanelRequestError` | Invalid input/JSON/limit; not dispatched |
| `PanelRequestBusyError` | Session not Ready, update pending, or Runtime slot occupied/rebuilding; not dispatched |
| `ExpiredPanelSession` | Session or activation/permission owner lost |
| `PanelRequestCancelledError` | New host command superseded this request |
| `PanelRequestTimeoutError` | 30-second post-dispatch deadline elapsed |
| `InvalidPanelResponseError` | Wrong ID/shape/JSON/size |
| `PanelRequestFailedError` | Handler threw/rejected |
| `PanelRequestUnavailableError` | Runtime/transport/internal failure |

One unresolved bridge call and one physical Runtime slot per plugin; enforce both in Host, not only JS. No automatic retry, panel waiting queue, AbortSignal, streaming or background callbacks. Unknown actions return plugin business errors. Missing optional request property is normal capability detection, not a Host error.

## Actual seams and minimal ownership model

| Existing seam | Change |
| --- | --- |
| `public_plugins/manifest.rs::PublicPanelV1`, CLI `src/manifest.ts`, generated schema/validator | Opt-in and 0.6.0 floor, historical minima unchanged |
| `public_plugins/runtime.rs::PUBLIC_RUNTIME_BOOTSTRAP_TEMPLATE`, `PluginCommandDispatch`, `PluginCommandCompletion` | Tagged internal dispatch, optional handler readiness, shared API/busy slot, bounded reply |
| `public_plugins/scheduler.rs::PluginRequestCandidate`, `PluginSubmissionOwner` | Explicit Command/PanelAction request and owner variants |
| `enqueue_reserved`, `retire_plugin_requests`, `complete`, `expire_timeouts` | Idle-only panel admission; exact-context retirement retaining physical slot |
| `public_plugins/activation.rs::enqueue_reserved`, `complete`, `request_network` | Manager admission/retirement under existing network transition and confirmed identity |
| `public_plugins.rs::PublicPluginService`, `dispatch`, `complete_submission_with`, `expire_runtime_timeouts` | Typed panel receiver/settlement, shared dispatch and lifecycle/timeout handling |
| `plugin_panel.rs::LiveSession`, `PluginPanelController`, `PUBLIC_PANEL_BOOTSTRAP_TEMPLATE` | Session-owned request registration/retirement, frozen bridge |
| `commands.rs`, `lib.rs` and command capability restrictions | One private async `plugin_panel_request`, panel-content caller only |

Common request identity remains plugin ID/generation/activation ID/admission epoch/Host request ID. Internal variant `Command` retains input and `PluginSubmissionOwner`; `PanelAction` carries action/payload and exact content label/session epoch. Do not fabricate launcher intent epochs/control text/submission tokens. Do not route action replies through command `parse_panel_response` or launcher publication. Panel Promise settlement is separate from `PublicSubmissionResult`, but all lifecycle iteration covers both owner kinds.

Use one small session-owned request registration/retirement handle, bound to the exact request context and a narrow service callback (weak service reference). A centralized `invalidate_session_locked` retires this registration before dropping/replacing the session. No event bus, separate worker or unbounded request history. The retirement callback does not emit, destroy WebViews, receive synchronously, or call back into controller.

Admission flow:

1. IPC derives caller from Tauri WebView label, parses epoch and validates bounded input. It never trusts identity inside payload.
2. Controller checks exact content label/epoch, phase Ready and no pending host update, holding its core lock through admission and registration. `begin_content_call` alone is insufficient because its returned identity is only a snapshot.
3. Manager checks confirmed enabled/installed/nonfaulted bundle, grant, opt-in and exact generation/activation/admission tuple; Runtime must be ready. Scheduler idle admission atomically rejects running/waiting/rebuilding without changing their state. Bind one pending response before dispatch.
4. Arm the bounded deadline observer before any emit/transport attempt, then emit the existing Runtime event with tagged invocation after releasing locks. An uncertain emit failure must never leave an occupied slot without a watchdog. Retirement before emit already expires the context; late replies cannot gain authority. Definite pre-emit failure can release an unstarted slot; uncertain delivery must retire and keep its slot until completion/deadline.
5. Host completion validates Runtime label/context/response and closes scheduler ownership. Release network/scheduler locks before controller access; under the controller lock recheck exact live session and clear only matching registration, then choose success or expiry and settle the pending receiver once. No success may be sent before this ownership recheck.

Implementation clarification: after physical completion removes the running slot, retain at most one exact-context completed-panel settlement ticket per plugin until the controller claims or discards the response. It carries response authority only, never a running slot or history. Existing credential/grant/lifecycle retirement and Host submissions must invalidate this ticket too. Claim under controller -> manager mutation (if used) -> network transition -> scheduler; release service authority locks before any controller entry. Every terminal settlement discards its matching ticket. This closes completion -> authority change -> response publication without changing legacy command behavior.

Do not await request inside `onUpdate` or `onHostKey`; existing acknowledgement deadlines stay unchanged. Ready admission returns Busy rather than wait for an acknowledgement blocked by its own caller. Initial plugin state comes from onCommand; content buttons trigger subsequent actions. Host Enter preserves existing command latest-wins semantics: retire the panel action, settle Cancelled, wait for physical completion/deadline, then dispatch command. A panel request never supersedes a running command.

## Cancellation and authority proof

Current `scheduler.cancel()` removes running while Runtime bootstrap still has `busy=true`; a newly emitted event can be silently dropped. Add exact-context logical retirement patterned on `retire_plugin_requests`: mark `current=false`, keep physical slot and original deadline, invalidate matching network context, settle once. Handler completion frees the slot; deadline expiry reuses Runtime replacement. Retirement itself is not a plugin fault.

Every removal/replacement uses the helper: `open_session`, `host_hidden`, `teardown_session`, `teardown_plugin`, `teardown_current`; include Escape/close/blur, Host Key failure, content ready/ack failure and unexpected content navigation/destruction. Document reload ends the old session; new content cannot inherit its epoch authority. Bootstrap retained request references expire when prepared epoch changes.

Existing plugin disable/uninstall/upgrade/authority changes/Runtime fault/shutdown already invalidate requests; extend settlement to panel receivers. Secret replacement/regrant must never revive old requests. Timeout/fault service paths currently tear down windows: add plugin + old-generation matched panel teardown so delayed failures cannot destroy a replacement panel.

Required lock order: controller core -> network transition -> scheduler; data API stays scheduler -> data-call gate/storage. Release network/scheduler locks before lifecycle panel cleanup. Never acquire panel from `with_current`, or hold pending-response map locks while acquiring panel. This makes close/replace and new admission serialize through the same controller lock, while API admission/retirement serialize through scheduler currentness.

After session invalidation returns, no new Runtime storage/settings/notification/network operation from that request can be admitted. An already admitted operation may finish; a committed save, scheduled notification or transmitted bytes cannot be rolled back. A cancelled save must not be labelled “not saved”; reload before deliberate retry. Existing data leases, exact network grants/hosts, broker cancellation, secret indirection and M4 epochs remain authoritative. Failure to acquire authority locks fails closed. No payload/credential logging.

Panel actions receive the current Runtime API only, including optional already-authorized HTTPS. There is no plaintext-secret getter or direct panel network API. The example is offline and declares only ui.panel. Captured APIs after completion/retirement remain expired.

## Runnable template editor

New `examples/public-plugins/com.uipilot.templates`, based on demo-panel structure and Notes layout; Notes stays unchanged. Stable ID, legacy `/templates`, minimum Host 0.6.0, runtimeActions true, only ui.panel; no remote assets/dependencies. HTML/CSS/JS live inside package; README/tests/manual cases outside.

Screen: Host search argument, 220px template sidebar, 48px toolbar/header, title/body editor, generated variable inputs, Preview/Save, selectable read-only preview and inline live status. Initially no selection; New Template explicitly creates a draft. Public CSS tokens, data-theme, labels, visible focus, stable loading, normal Tab order and both themes are mandatory. Host Enter filters names without overwriting dirty editor. Escape/blur retain Host teardown; show unsaved state, no unreliable close veto/autosave promise.

Syntax: only `{{name}}`, ASCII `[A-Za-z][A-Za-z0-9_]{0,31}`, optional surrounding whitespace. Runtime discovers distinct names in first-use order, rejects malformed braces, substitutes literal values and reports missing values. No eval, expressions, loops, HTML, partials, date macros or recursive expansion. Use textContent/textarea rendering.

Runtime owns all catalog I/O and business logic:

- `load` payload: `{id:string}`; retrieve one saved template and revision.
- `preview` payload: `{body:string,values:Array<{name:string,value:string}>}`; return variables, missing values and rendered text; no write.
- `save` payload: `{id:string|null,expectedRevision:number|null,title:string,body:string,values:Array<{name:string,value:string}>}`. New draft uses both null; update requires existing ID and exact safe-integer revision. Persist one catalog snapshot, return ID/revision and list metadata. Missing sample values may be saved; malformed syntax may not. Stale revision returns conflict, never silent overwrite.

Values use an array, not variable-keyed JSON records: names such as constructor/prototype are ordinary strings and remain valid template variables. Reject duplicate names, wrong shapes and invalid names; discard well-formed values for names no longer used by the body and return normalized samples. A missing array entry means missing value; an explicit empty string means deliberate empty substitution. UI does not insert an empty entry for an untouched input, but retains one after the user explicitly clears it. Save permits absent entries; preview reports them as missing. All action payloads reject unknown top-level fields.

OnCommand returns list metadata/filter only. Storage key `templates.catalog-v1` holds schema version, monotonic next ID and <=20 templates. Each has stable string ID, safe-integer revision, title <=80 Unicode characters, body <=8 KiB UTF-8, <=16 variables, sample value <=256 characters; preview <=32 KiB UTF-8. Counter exhaustion rejects. Validate loaded data; corrupt data produces a visible error and is never silently reset. Catalog compact UTF-8 serialization is additionally capped at 512 KiB before storage.set; capacity failure is a business error and preserves the draft and prior catalog. Actual storage.rs has a 5 MiB per-plugin serialized-values quota and no separate per-value byte cap; this aggregate cap stays below it even though worst-case JSON escaping may reach the application limit before 20 templates. Storage/transport failure never reports saved; reload after uncertain persistence. Whole validated catalog uses one storage.set; no separate metadata write, migration/import/export/delete required.

UI tracks a draft edit revision per request and never applies an old reply to newer text or marks newer typing saved. Disable duplicate Preview/Save; preserve draft on error. Ambiguous save offers explicit Reload Saved, never automatic retry. Saved template/samples survive restart. Preview supports ordinary selection/copy; no new clipboard API/button is promised. To discover variable inputs, Preview may return missing-variable metadata before all values exist.

## Two cohesive tasks and verification

1. Integrated Host capability: contract/schema/CLI/readiness plus tagged request, scheduler/bridge, bounded response, exact retirement and generation-matched teardown, with the focused authority/concurrency regressions below. Sync Host/CLI release constants once for 0.6.0 in this integrated change, keep historical compatibility minima. Update SDK docs: content can request its own Runtime while direct networking remains forbidden.
2. Runnable offline example and delivery: editor/business logic/package and its focused parser/persistence checks, plus concise visible labels for the two existing install controls. Preserve handlers and compact 720px layout. Produce new Host build and packaged plugin; no separate label-fix release/test cycle.
Delivery verification and user handoff: build/typecheck and narrow regressions, then exact artifact paths/version and human cases. User runs primary native acceptance. Fix observed failures with focused rechecks; do not postpone first usable handoff for repeated broad suites or per-file TDD ceremony.

Minimum automatic evidence: opt-in CLI/Host parity and missing-handler readiness; invalid caller/epoch/activation/JSON/reply ID; Busy leaves current slot untouched; barrier-controlled close/admission/late-write race; exact retire keeps physical slot until completion; host submit does not overlap; duplicate/late replies settle once; timeout cannot kill new generation; relevant existing M4/admission tests; template parser/conflict and failed-save persistence checks. Cover centralized session removal entries without duplicating entire suites. Source-string assertions alone do not prove these races.

Human preconditions: supplied Host 0.6.0, template package, existing Notes/M2-A packages. No real credentials. Agent supplies numbered cases, user returns PASS/FAIL plus observed outcome; never claim human PASS before response.

| Case | Steps | Expected |
| --- | --- | --- |
| Install discovery | Open plugins settings and inspect both install buttons; install supplied package | Labels clearly distinguish package from directory; no icon guessing |
| First use | Open `/templates`, New Template, title Greeting, body `Hello {{name}}`, Preview | Initial selection empty; variable name field appears; missing value explained; draft retained |
| Preview/save | Enter Ada, Preview, Save, reopen saved template | Exact Hello Ada; saved body/sample reload; no extra window/tag replacement |
| Persistence | Exit/restart Host, open saved template, edit/save and reopen | Stable template ID/content and samples persist |
| Validation | Preview `Hello {{`; fix; try empty title; fix/save | Useful inline errors, no draft loss, recovery works |
| Rapid/close | Repeated Preview/Save, type during reply, Escape/reopen | No duplicate work/stale UI; newer typing not marked saved; reopened data explains saved state |
| Keyboard/themes | Core workflow Light/Dark with Tab/Shift+Tab/Enter/Escape and long content | Readable labels/focus/status, no overlap, correct Host close |
| Compatibility | Existing Notes plus M2-A live/submit/copy/favorites | Existing behavior works, no result-action dependency |

Done means runnable delivery plus user native acceptance, not a design/mock/test count. Design review does not constitute implementation or manual acceptance.

