# Public plugin command hotkeys

## Scope and outcome

This host increment lets a user assign one global chord to one installed public-plugin command, identified by `pluginId + commandId`. Configuration lives in that command's existing Settings row. It works locally and survives restart and command rename.

Deliver Host **0.11.0**. Keep public plugin API v1, the public-plugin state schema, existing example minimum-host versions, and ordinary Enter behavior unchanged. Synchronize the CLI's current host target only. No command double-tap, new demo plugin, generic shortcut framework, input synthesis, cloud service, or new system capability.

## Chosen approach

Use a defaulted host Settings map and a dedicated command registration table. Reuse the launcher recorder, lifecycle show transport, and command activation paths. The launcher-only `RuntimeSettings.registered` must remain launcher-only: its rollback deliberately unregisters other entries in that vector.

Putting bindings in strict public-plugin state would broaden the migration contract; treating every native callback as a launcher show loses command identity and breaks recording. Neither is needed here.

## Settings and visible behavior

- Persist `publicPluginCommandHotkeys: BTreeMap<pluginId, BTreeMap<commandId, canonicalChord>>`, defaulting to empty. Existing Settings updates preserve this map.
- Add main-window-only `set_public_plugin_command_hotkey({ pluginId, commandId, hotkey: string | null })`; null explicitly clears. Reject unknown fields/identities and non-chord values before native work. Use `HotkeyKind::parse/canonical`; launcher double-tap remains supported.
- Enrich inventory command rows with `hotkey: string | null` and `hotkeyStatus: 'unset' | 'active' | 'suspended' | 'unavailable'`. Suspended means disabled/faulted; unavailable means native registration/cleanup could not be confirmed. Expose an independent host `hotkeyRevision`; row continuation ownership includes both inventory and hotkey revisions.
- Extract the existing recorder into a small shared component accepting value, commit callback, and allow-double-tap option. Command rows disallow double-tap and add an accessible explicit “清除” action. Reuse the row's mutation/reload/focus recovery path; failed reload retries the read only.
- Record/edit/clear, loading, errors, and long chords fit the existing compact row in both themes using semantic tokens. Escape cancels recording; Tab order and focus return remain predictable. Keep a binding visible while suspended.
- Canonical duplicates conflict with the launcher chord and every saved command chord, including suspended ones; changing the launcher checks the same map. Native registration failure reports an unavailable/conflicting system shortcut without guessing its owner. An OS may refuse reserved chords without exposing which app owns them.

## Registration, persistence, and locks

Use a shared hotkey-operation mutex in LifecycleCoordinator around launcher edits, command edits, and command reconciliation. Keep a separate native command table keyed by shortcut ID with stable command target, canonical chord, and confirmed/uncertain state. The existing critical reservation protects work from exit; it is not this serialization gate.

A command edit runs in a blocking worker: authorize main first; reserve critical work; acquire the shared gate; read current Settings and installed command identity; reject conflicts; register replacement; unregister old; revalidate installed identity and persist only that map entry in one brief manager mutation-lock scope; publish dispatch mapping and bump hotkey revision. Clearing skips replacement registration. Do not use a stale whole-Settings snapshot for persistence.

If native or persistence work fails, keep the old durable preference and restore the captured registrations. Failed rollback leaves affected entries unavailable and ineligible for dispatch; retain tracking of possibly registered IDs so retry/exit can clean them. A callback copied before an edit must match the current binding revision before activation.

Lock order is shared hotkey gate, then brief public-manager authority access and/or Settings persistence; the native-table lock is never nested with public authority. Never hold public-manager locks during native registration, never hold the native table lock while acquiring public authority or dispatching, and never call a gate-taking reconciler while holding a public mutation lock. Launcher native operations remain serialized through the same gate. Dispatch copies its candidate and releases the table lock before validation.

Public lifecycle publication is authoritative even if native cleanup lags: every command dispatch revalidates installed/enabled/nonfaulted state, command existence, and current bundle authority. Disable/fault closes execution authority before post-lock unregister reconciliation; preferences remain for re-enable.

Startup initializes public service first, prunes against committed installed command IDs, registers the launcher, then independently reconciles command bindings. One bad command chord is unavailable and does not prevent other chords or host startup.

After install/update, enable/disable, uninstall, and runtime-fault publication, reconcile from a fresh authoritative inventory outside manager locks, including faults raised by runtime paths rather than only management IPC. Serialize reconciliation with edits; a fresh inventory snapshot must be revalidated if lifecycle publication races its registration.

For cross-store cleanup, public-plugin metadata remains the authoritative installed-command set. After update, retain surviving IDs and prune removed IDs; after uninstall, clear all that plugin's host bindings even when retainData is true. Extend `finish_uninstall_cleanup` for durable host cleanup. If pruning fails, return/retain cleanup failure and suppress removed identities; do not report successful cleanup.

Before committing any subsequent package install/update, prune against the currently committed installed-command set and require that write to succeed. Startup performs the same prune before registration. Thus a failed post-update prune or a crash after uninstall cannot resurrect a stale binding when a removed ID is later reintroduced. Reconcile again after commit against the new set. Do not claim cross-file atomicity or roll back a committed plugin package because native registration fails.

## Native delivery and recording

Resolve each pressed native shortcut by its actual shortcut ID before dispatch. Ignore release/untracked/uncertain entries. Launcher behavior retains its existing foreground policy and modifier double-tap hook.

For command shortcuts while main owns foreground, emit a main-only native hotkey event carrying the canonical chord, stable target, and binding revision. The frontend checks recording before any navigation or command activation: the active shared recorder consumes the canonical chord, including a chord already registered to another command or the launcher. No plugin executes and Settings stays open. Preserve the existing completion grace protection for delayed native events and deduplicate DOM/native capture of one press. Launcher recorder double-tap events retain their existing cancellation behavior.

A hidden native press can arrive while a Settings recorder remains mounted: the shown payload therefore also carries canonical chord and binding revision with the command target. Before transitionView, shown checks recording/completion grace and offers the chord to the recorder, then returns without command activation or Settings navigation. The native show may reveal main, but cannot run the command. Preserve launcher shown suppression in the same branch.

When not recording, a foreground command event enters the common command-entry path using the current invocation. When hidden, request launcher show with the stable target. Extend the existing show request/readiness slot and `LauncherShown` payload together; the pending request carries identity, not merely `ShowTarget`. Keep last-request-wins behavior and emit identity with the same invocation as the show. The optional command-entry object is exactly { pluginId, commandId, hotkey, bindingRevision }; validate all fields. Update strict parsing for that optional object; do not emit a second activation event that can race readiness.

Both foreground and shown entry use the same frontend operation. Capture invocation/view epoch and an entry token before resolving; after every await, discard replies if another show, entry, navigation, hide, or disposal superseded that owner. Clear command-entry identity on those transitions. A late foreground event from an obsolete binding is rejected before execution.

## Stable command entry and execution

Add a main-only resolve-entry IPC for the stable target. Under the existing manager authority checks, resolve the current command label, output/activation modes, input requirement/placeholder, and activation metadata. Factor the current `route` construction so both name routing and ID routing share installed/enabled/fault/generation/bundle checks.

The entry target remains attached to command-originated searches and submissions, including Window commands. Add an optional host-internal stable command target to that path and route by IDs plus the argument, never by a previously resolved name. A renamed label is presentation only. Existing public scheduler admission/publication guards revalidate bundle authority; disabled, removed, replaced, or stale work cannot run another command. Plain typed searches and ordinary Enter retain their existing path.

For empty required input, arm the command and focus its argument prompt without invoking the plugin or opening a Window. Apply this rule to hotkey-originated entry before the existing Window completion-commit exception; do not change that exception for ordinary launcher completion.

For no-input commands, enter and run the command once: MainResult uses its existing commit/query flow to load results, Window uses existing commit/transfer, and Panel uses existing panel opening. A hotkey never executes the first result action; it only invokes the command. This hotkey-specific commit leaves ordinary Enter and completion-preview semantics unchanged. Subsequent input/submit remains owned by the stable target and current invocation. Panel identity/session handling stays on its existing path.

## Focused evidence and manual acceptance

Automated tests use fake native registration only; do not install global chords, synthesize input, change user plugin data, or run real host interaction.

- Settings defaults/round-trip/preservation, canonical conflicts both directions, explicit clear, rollback after persistence failure, uncertain rollback, and startup isolation of a bad chord.
- Concurrent command A and launcher edit request the same chord: shared-gate order admits one and rejects the other with no duplicate dispatch mapping.
- Save snapshots an installed ID, then uninstall publishes before save persistence: revalidation rejects it; post-lock reconciliation removes native dispatch and durable cleanup cannot be bypassed by reinstall.
- Runtime fault unregisters without deleting preference; re-enable restores it. Update retains surviving IDs and removes deleted IDs. Uninstall with either retainData value clears bindings; failed prune plus later reintroduction cannot resurrect one.
- Hidden command A queued before readiness, then command B/tray show replaces it: only the last request's identity is delivered. A newer invocation or navigation while resolve awaits prevents the old reply from applying.
- Rename command A and reuse its old label for B between resolution and search: the hotkey still targets A by ID or fails closed. Required-input Window never dispatches empty; ordinary Enter fixtures remain unchanged.
- Registered native chord during recording, and its delayed event after DOM completion: one recording result, no command execution/navigation. Reject non-main callers before protected state access.
- Row clear/error/reload and keyboard focus; strict protocol shapes and ownership; no new plugin fixtures.

Human acceptance uses existing Text Snippet and Notes packages: assign/edit/clear, trigger while hidden and visible, required-input focus, Notes panel entry, launcher/command/system conflicts, rename, disable/re-enable, restart, update and uninstall cleanup. Use existing temporary package copies where lifecycle cases need variants; no new example. Check Light/Dark and keyboard accessibility. The agent supplies a built executable and a short checklist, and reports actual global-trigger/restart verification as pending until performed.


