# Host 0.12.0 — explicit browser URL result actions

Date: 2026-10-26. Baseline: accepted 0.11.0, source f2612ad, documentation HEAD 4d04eafbfce93ace8a1af681eadf53bcc7c594e5. This is a bounded M5 system capability under the user's standing authorization to plan and delegate development autonomously. Real browser/OS tests belong to the user. Existing command-hotkey core acceptance remains valid.

## Outcome and choice

A plugin main-result action can ask the Windows default browser to open one absolute HTTP(S) URL when the user explicitly chooses that action. Reuse the existing main-result action menu, immutable private snapshot, permission consent, and browser adapter. Extend the existing text-snippet plugin to demonstrate the workflow without another demo package.

Alternatives considered: a standard Panel component kit improves reusable UI but needs several components and broader keyboard/form contracts; user-selected file access requires selection grants, read limits and new native-dialog lifetime rules. URL result actions are the smallest useful next vertical and reuse a tested execution boundary. Full UI components, selected-file access, paste, arbitrary protocol/Shell execution, direct Runtime opening, Panel/Window APIs, network grants and online distribution remain outside this slice.

## Manifest and version contract

Host and CLI target become 0.12.0, including PublicPluginHost::current (currently hardcoded 0.10.0) and its assertion; schema/api version and persisted state format remain unchanged. Add permission `url.open`, available only on Windows. A declaring package must have `minimumHostVersion >= 0.12.0`, exactly Windows supportedPlatforms, `runtime.resultActions: true` and mainResult commands (existing resultActions rules). Consent and details describe opening HTTP(S) links in the default browser. Existing upgrade permission confirmation applies: rejecting the new grant leaves the old installed plugin/data/bindings intact. Do not grant silently or treat network.https as authorization.

Old packages remain valid, and their historical feature minima stay unchanged. Continue requiring callable onAction when runtime.resultActions is true, including static-only packages. No new bootstrap opt-in or direct open API.

## Result contract and URL validation

Extend PluginResultAction with exactly `{id, title, type: "openUrl", url: string}`. Existing action/result count, identifier/title limits, JSON/response bounds, defaultActionId validation and extra-field rejection apply. Legacy defaultAction remains copyText only. The new action may be an explicit default in third-party packages; it never executes on query/publication alone. The sample keeps copy as its default.

Validate at response parsing and immediately before the native effect using a shared strict host function that returns canonical `url::Url`. Require raw and canonical UTF-8 length <=4096 bytes, explicit case-insensitive `http://` or `https://` prefix, a nonempty raw authority immediately after that prefix (before the next slash, question mark or hash) with no raw @ character, parsed nonempty host and no username/password, no literal whitespace, C0/C1 controls, DEL or backslash. Reject relative/opaque URLs, file/javascript/data/custom schemes, malformed hosts/ports and credential-bearing URLs. Never trim or repair invalid text into a destination. Percent-escaped whitespace/control bytes remain allowed as URL data; only literal characters are rejected. Unicode domains, normal URL escapes, queries and fragments are accepted when the parser and these bounds permit them. The validated canonical URL is passed as a URL object to existing browser_open::open_url; never create a shell command string.

This permission authorizes opening the validated initial URL externally. Browser redirects, authentication and browser network policy are owned by the default browser; this is not a Host HTTPS request or a promise to constrain redirects. No raw URL is added to diagnostic logs/error text.

## Admission, authority and effect ordering

Preserve all contracts in 2026-10-25-result-actions-design.md: Main caller boundary, opaque request/result/action handles, plugin/command/generation/activation/admission identity, current query/publication ticket and lease, the snapshot-wide Available/InFlight/Consumed claim and captured UI owner. The renderer receives action IDs/titles only, never URL authority. Do not trust payload/type/URL sent by the renderer.

Extend existing snapshot grant requirements to include URL permission wherever action sets are published, resolved, admitted and replaced. Preserve the existing snapshot-wide copy requirement; a mixed snapshot is not weakened to checking only the selected action. Require both declaration and current grant for each capability used by the snapshot, and recheck at effect admission. Old snapshots become unusable on disable, reload, uninstall, permission or generation change, query/show/hide retirement as before. Invalid response/action must not reach the native adapter.

Use the existing static copy-action admission shape for URL opening: retain legacy/public admission and manager mutation checks, scheduler publication check and registry claim before the synchronous native callback. Registry/scheduler internal locks are released before invoking the browser adapter, as in copy; manager mutation ownership protects grant/generation changes through the effect. Do not add scheduler Runtime work, timers, retries or a new controller. A query may retire the source after an effect has been admitted; that cannot undo an OS launch already requested, and no later completion may overwrite the new UI owner.

Pre-claim failure does not consume except existing replay/in-progress classification. After successful claim, both adapter success and adapter failure consume the entire source snapshot, preventing duplicate launch through any sibling action. No automatic retry; the existing explicit rerun creates fresh user intent. The adapter success means Windows accepted the request, not that a page finished loading.

## Frontend outcome and feedback

Add `{kind: "urlOpened"}` to ResultActionOutcome and `urlOpenFailed` to its fixed error-code set. On owned success, close the action menu, mark the source consumed and show `已请求打开链接`. Keep the existing launcher lifetime behavior; no new hide/focus-restoration path. Browser focus transfer follows the existing native browser adapter/normal window behavior. On admitted adapter failure, show safe fixed failure feedback with rerun available and source consumed. Invalid/stale/ungranted cases retain existing typed failure policy. A stale response never updates another command or result set.

Use the existing actions menu/footer, status surface, keyboard and theme behavior in docs/ui-guidelines.md. No special browser UI or decorative component.

## Existing example and docs

Upgrade com.uipilot.text-snippet to 1.1.0, minHost0.12.0, adding url.open. Preserve pluginId, both commandIds, storage key/schema, settings and default copy/save/read behavior. Add secondary `在浏览器打开` only to the original transform row and saved latest row when their exact text is a valid supported URL. Do not add it to the uppercase transformation row, since path case can alter destinations. Ordinary text and invalid/dangerous URL-like text retain normal copy/save rows without the open action. No automatic network/browser operation. Keep onAction, even if a given response only has static actions.

Update public SDK types and contract docs/tutorial, generated schema/CLI copies and host target/version expectations. Use a small shared local URL predicate in the example if needed; host remains authoritative. Tests cover the sample without invoking the browser. No new dependencies.

## Evidence and manual handoff

Focused automated evidence covers valid canonicalization and bounded invalid URLs; Host/CLI permission/version/platform parity; no grant/stale/disabled/generation-changed source produces zero opener calls; one snapshot admits at most one opener call; failed adapter consumes without replay; source retirement/newer UI suppresses old completion; runtime replacement retains full snapshot requirements; existing copy/runtime paths remain valid. Include a user-scope installation permission regression using existing tests where feasible, not a new install harness.

Use fake opener callbacks and temporary state. No actual browser/network, input injection, clipboard, app or user-data operations by agents. Run directly affected tests, typecheck/build and necessary schema generation checks, not the root historic verification matrix. Known unrelated credential tests and Enter-count behavior stay deferred.

Deliver a separate 0.12.0 acceptance kit using the existing acceptance identity, unchanged resources, updated text-snippet archive and concise manual steps. Preserve every prior binary/package. First manual group: update consent, input https://example.com, choose open via Ctrl+K, verify one default-browser request and rerun behavior; verify ordinary text copy/save and original bindings/data. Next negative group checks unsupported URLs offer no action and denying update permissions preserves the old install. Manual outcomes remain pending until the user reports them.