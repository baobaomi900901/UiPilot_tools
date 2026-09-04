# UiPilot Managed Everything Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `subagent-driven-development` (recommended) or `executing-plans` to implement this plan task-by-task. Use `requesting-code-review` after each task and `verification-before-completion` before reporting it complete.

**Goal:** Make the installed Windows build start and own a private Everything client automatically, backed by a UiPilot-managed Everything Service, so `/find` works for the desktop user without a separate Everything installation.

**Architecture:** The elevated NSIS transaction securely stages a Setup-embedded native Rust Helper; that Helper owns strict state parsing, signed-source validation, the machine-wide Service transaction, durable recovery, and uninstall cleanup. A production-only Rust supervisor owns the per-user/per-session hidden client, Job Object, readiness state, and PID-bound IPC connection. Existing Query2 payload handling, ResultRegistry behavior, and authenticated file execution remain unchanged. A release gate combines deterministic source/unit checks with an explicit Windows VM matrix.

**Tech Stack:** Tauri CLI 2.11.4, NSIS, Everything 1.4.1.1032 x64 Stable, Rust 2021, `windows` 0.61.3, PowerShell 5.1+, React 19, TypeScript 7, Vitest 4.

**Source Spec:** [Approved managed Everything lifecycle design](../specs/2026-09-04-managed-everything-lifecycle-design.md)

## Global Constraints

- Windows x64 installed builds are in scope. Development startup behavior remains on the existing developer scripts unless a task explicitly says otherwise.
- Use only the private identities fixed by the design: instance `UiPilot`, Service `Everything (UiPilot)`, and pipe `\\.\PIPE\UiPilotEverythingService`.
- Never discover, connect to, modify, stop, or uninstall the user's default Everything instance, Service, configuration, database, or process.
- Keep the Service executable and `managed-runtime.json` beneath the protected `%ProgramData%\UiPilot_tools\everything` root. Keep client INI/DB beneath the current user's `%LOCALAPPDATA%\UiPilot_tools\everything\<session-id>` root.
- Treat the approved SDDL, desktop shell SID acquisition, signed-resource validation, manifest matching, and fail-closed ownership checks as security boundaries. Do not replace them with usernames, environment variables, active-console guesses, process names, or install-directory trust.
- The native `uipilot-everything-installer.exe` is an `asInvoker` Setup payload only: never install, register, download, discover, or invoke it from the UiPilot runtime, PowerShell, cmd, WMI, PATH, ProgramData, or a previous staging directory.
- Serialize every install/uninstall transaction with `Global\UiPilotManagedEverythingInstaller`, `Global\UiPilotManagedEverythingInstallerActivity`, the exact BA/SY descriptors in the design, and operation/transaction IDs. Validate existing named objects before reading managed state; mismatches fail closed.
- Treat `$INSTDIR` only as an untrusted read-only bundle source. Hold non-write/non-delete-sharing handles through identity/hash/version/signature checks and same-handle copy; derive every Service, ProgramData, manifest, journal, and deletion target from fixed constants or validated protected state.
- Keep startup non-blocking. Everything readiness or failure must not delay the main window, tray, calculator, plugins, quick links, or settings.
- Keep ResultRegistry semantics unchanged: client restart alone does not invalidate published actions; the next admitted query retires the old set; a failed query leaves the set empty; execution still reauthenticates every path component.
- Keep private instance, pipe, INI, DB, process arguments, and runtime generation entirely Rust/installer-owned; the WebView must not submit or override them.
- Log stable reason codes only. Do not log searches, result paths, SID values, pipe names, private filesystem paths, or raw system errors to user-visible surfaces.
- Do not run installer, uninstaller, Service, ACL, token, or multi-user mutation tests on the developer machine without explicit user approval. Use disposable Windows VMs for the release matrix.
- The existing uncommitted `src-tauri/tauri.conf.json` `perMachine` change belongs to this feature. Task 1 may absorb that exact change; do not stage unrelated worktree changes.

## Core Contract Overview

- `managed-runtime.json` is the authoritative machine-owned record. It contains the fixed product identity plus version, architecture, SHA-256, Authenticode publisher, canonical Service executable, `desktop_user_sid`, and expanded SDDL. Installer repair, upgrade, rollback, and uninstall act only after both this manifest and live SCM/filesystem state match.
- `uipilot-everything-installer.exe` accepts only the fixed actions `prepare-install`, `recover-install`, `commit-install`, `rollback-install`, `pre-uninstall`, and `post-uninstall`. A protected staging journal carries the current `operation_id`; durable `managed-transaction.json` carries the interrupted `transaction_id`; recovery explicitly validates both IDs before state changes.
- NSIS holds the validated machine mutex and activity event across the complete transaction. Each Helper validates its high-integrity fully elevated BA token, staging owner/DACL/non-reparse identity, parent Setup PID plus creation time, and the still-live activity lease before reading requests or causing side effects.
- `EverythingRuntimeSupervisor` exposes only `Starting`, `Ready`, `Unavailable(reason)`, and `Stopping`, plus a ready-client lease containing an internal generation and PID-bound client. Query failure may invalidate a lease only when its generation is still current. Process handles, Job Object, generation values, retry accounting, paths, and diagnostic details stay out of ResultRegistry and frontend contracts.
- `EverythingClient::connect_owned(instance, expected_pid, timeout)` accepts an IPC window only when `GetWindowThreadProcessId` returns the expected child PID. Every reconnect repeats this binding. Database-loaded and fixed Query2-capability probes are explicit readiness checks; `total == 0` is valid.
- Shutdown is idempotent: first reject new searches, then close the cached IPC client, gracefully stop and bounded-wait for the owned child, terminate the exact child only if needed, and finally close the Job handle. The machine Service remains running.

## Global Execution Rules

1. Implement Tasks 1–3 in order; each task ends in one atomic commit and a review gate before the dependent task starts.
2. Every task follows TDD: add focused failing tests, confirm the intended failure, implement the minimum contract, rerun the focused tests, and commit only green behavior. Do not commit skipped security assertions.
3. After each implementation commit, launch an independent review agent against that commit and the approved design. Resolve actionable findings and repeat independent review until it reports no actionable issue, as the user requested.
4. Keep the custom NSIS template byte-for-byte aligned with the Tauri CLI 2.11.4 upstream template outside the three approved behavior regions. Any future CLI upgrade must fail the source gate until the template is explicitly rebased.
5. Record Windows VM evidence outside Git under `artifacts/managed-everything/`; do not commit machine names, usernames, SIDs, local paths, or raw logs containing them.
6. If a required Windows token/session topology cannot be created, report the unexecuted matrix row; never substitute a weaker assertion and mark it passed.

---

### Task 1: Implement the Transactional NSIS Service Lifecycle

**Files:**

- Create: `src-tauri/src/bin/uipilot-everything-installer/main.rs`
- Create: `src-tauri/src/bin/uipilot-everything-installer/model.rs`
- Create: `src-tauri/src/bin/uipilot-everything-installer/transaction.rs`
- Create: `src-tauri/src/bin/uipilot-everything-installer/windows.rs`
- Create: `src-tauri/windows/installer.nsi`
- Create: `src-tauri/windows/nsis-hooks.nsh`
- Create: `src-tauri/resources/everything/Everything-UiPilot.ini`
- Create: `src-tauri/tauri.vm-instrumented.conf.json`
- Create: `scripts/build-managed-everything-installer.ps1`
- Create: `scripts/test-managed-everything-installer.ps1`
- Modify: `scripts/test-everything-package.ps1`
- Modify: `package.json`
- Modify: `src-tauri/build.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/tauri.conf.json`

**Dependencies:** None. Follow design sections `固定身份与路径`, `子系统 1：NSIS Service 生命周期 / 原生安装器 Helper 边界`, and `子系统 3：验证与发布门禁`. This task consumes the reviewed Everything executable, license, and lock already under `src-tauri/resources/everything/`.

**Distinct test coverage:**

- Snapshot/source gate proves the custom template is derived from Tauri CLI 2.11.4 and differs only in the approved reinstall flow, uninstall gate ordering, and full-lifetime Helper staging/handoff region.
- Helper model tests cover strict unknown/duplicate-field rejection, embedded lock/License/INI bytes, canonical fixed roots, semantic SCM arguments, expanded SDDL, same-session Explorer SID selection, upgrade SID preservation, operation/recovered-transaction ID rules, stable exit codes, and foreign/ambiguous-state rejection.
- Native boundary tests cover `asInvoker`, fully elevated high-integrity enabled-BA admission, parent PID/creation-time checks, exact staging owner/DACL, direct non-reparse request/journal files, existing mutex/activity-event type/owner/DACL checks, and standard-user direct invocation failing before input reads.
- Transaction tests cover fresh bootstrap residue, durable phase flush/recovery, parent Setup death while Helper retains the activity lease, concurrent Setup/uninstaller rejection, same-handle untrusted `$INSTDIR` source copy, auto-update, manual replacement, repair, explicit uninstall, Service switch/rollback failures, and manifest-last idempotent uninstall continuation.
- Given a correctly described machine mutex that the parent Setup does not own, Helper's nonblocking wait acquires and releases it, then fails before parsing a malformed request; no staging, ProgramData, SCM, or manifest state changes.
- Given no Service/manifest/version directory, an exact protected empty managed root or root containing one valid transaction temp is recoverable; adding any extra entry or changing owner/DACL leaves every byte and Service state unchanged and returns fail-closed.
- Given a writable `$INSTDIR`, a pre-existing incompatible writer causes share violation before any protected-state change. If the source handle is already fixed and a concurrent replacement is attempted, the protected destination's bytes and file identity come only from that held handle and the replacement is never installed or executed.
- When failure is injected immediately after old-Service stop, new-Service install, and new-Service start, rollback removes only a matching new Service, reinstalls and starts the verified old Service, and retains the old manifest. When rollback cannot prove one old/new state, it retains all verified binaries/manifests/journals and terminates fail-closed.
- When the parent Setup dies while a mutating Helper is active, the old activity lease keeps a new Setup busy; after the old Helper rolls back/exits, the new operation adopts only the validated recovered transaction ID and converges before app-file copy. When uninstall stops after each referenced-directory deletion, rerun continues from the still-present manifest and deletes that manifest last.
- Source assertions prove the fixed Service/instance/pipe/SDDL, no script interpreter, no installed/runtime Helper, pre-bundle x64 release build, three template regions, fixed action protocol, Service-running check, and protected manifest/journal locations.

**Implementation points:**

- [ ] Implement the standalone `uipilot-everything-installer` binary modules: fixed CLI actions in `main.rs`, strict persisted/request models and embedded reviewed bytes in `model.rs`, deterministic phase/recovery decisions in `transaction.rs`, and handle-owning Win32 effects in `windows.rs`. Do not expose the module from the UiPilot library or Tauri commands.
- [ ] Declare the Helper bin explicitly and have `src-tauri/build.rs` emit a bin-specific MSVC `/MANIFESTUAC:level='asInvoker' uiAccess='false'` linker argument while preserving the existing Tauri app-manifest build. Source/build tests must inspect the resulting Helper manifest before NSIS packaging.
- [ ] Add the explicit x64 release Helper build script and invoke its production mode from the Tauri pre-build/package path. Fail before NSIS when architecture, manifest, artifact, or embedded-resource inputs differ; do not copy the Helper into installed resources.
- [ ] Vendor the exact Tauri CLI 2.11.4 `installer.nsi`. Mark the three permitted regions and implement in-place replacement/repair, explicit same-version uninstall-and-exit, app-running-before-preuninstall, and one staging/activity lifetime spanning every install or uninstall hook plus rollback.
- [ ] In NSIS, validate and acquire the exact machine mutex first; validate that no activity event exists, treating a trusted existing event as busy and any untrusted named object as fail-closed; only then atomically create the elevated account's LocalAppData Temp staging and activity event with the exact protected descriptors. Hold both named-object handles across every hook, Helper, file-copy, rollback, and cleanup step.
- [ ] Generate a fresh unpredictable 128-bit `operation_id` with `CoCreateGuid` for each Setup/uninstaller transaction; extract only the embedded Helper; pass staging paths, parent PID/creation time, and commit-only `$INSTDIR`; map only stable exit codes; perform bounded cleanup without installing the Helper. Only commit/rollback within the same transaction require operation/transaction IDs to match; recovery carries a separately validated old ID.
- [ ] Make Helper admission fail before request/journal reads unless it is `asInvoker`, fully elevated/high integrity with enabled BA, its executable/request/journal are direct non-reparse staging files, owner/DACL match the current TokenUser/BA/SY contract, the exact parent process remains alive, the trusted activity event is held, and nonblocking wait on the machine mutex proves ownership remains with another process. If the Helper can acquire the mutex, release it and fail before input reads.
- [ ] Make `prepare-install` read-only with respect to Service, ProgramData, and app files. Validate current manifest/SCM/binary state, preserve an upgrade SID, resolve a fresh SID only from same-session Explorer, and report recoverable durable/bootstrap states without mutating them.
- [ ] Implement `recover-install` before Tauri file copy. Validate current operation ID plus recovered durable transaction ID, activity lease, protected journal/root, old/new binaries, and live SCM; converge only an unambiguous phase, then rerun prepare. Reject foreign mutex, Service, directory contents, journal, ACL, or identity without takeover.
- [ ] Make `commit-install` open every `$INSTDIR` source without write/delete sharing, retain source handles across final-path/file-ID and type checks, validate Everything against embedded lock version/hash/publisher, compare bundled lock/License/INI to embedded bytes, copy Everything from the same handle, and fully revalidate the protected target before any Service install.
- [ ] Before the first ProgramData/Service side effect, atomically create the BA-owned protected managed root and durable transaction journal; persist/flush each next phase. Cover the exact empty/single-temp bootstrap residue and reject every broader missing-manifest directory.
- [ ] Commit the Service switch in order: validate old ownership; stop/remove only a matching old private Service; install the new fixed Service; validate canonical SCM ImagePath/arguments and Running; atomically replace runtime manifest; then clean only unreferenced verified versions. On parent death or failure, begin no new phase, perform idempotent rollback, and retain all verified recovery material if convergence cannot be proven.
- [ ] Make true uninstall run only after the app-running gate and only when `$UpdateMode != 1`. Recover a pending install first; treat valid-manifest/service-absent as an idempotent intermediate; remove only a matching private Service; delete referenced directories first and runtime manifest last; allow safe repeated post-uninstall without directory enumeration expansion.
- [ ] Add deterministic fault checkpoints behind a Cargo feature disabled in production Helper builds. The instrumented Helper exposes named checkpoints for bootstrap temp create/flush/rename, each Service phase, rollback, each referenced-directory deletion, and manifest deletion. Add a separate Tauri config/build mode that embeds it only into a visibly marked, disposable-marker-gated, independently named VM Instrumented Setup under `src-tauri/target/vm-instrumented/`; this Setup is never a release artifact, and scripts must never invoke the Helper directly.
- [ ] Make the VM Instrumented Setup build reproducible without installation and make Source gates reject it from the production bundle/output path. Every normal Tauri build must rebuild and inspect the feature-off Helper before NSIS; interrupted VM builds must not leave a feature-on artifact eligible for a later production Setup.
- [ ] Generate the INI template with the fixed private instance/pipe contract and disable tray UI, updates, self-update, HTTP, ETP, FTP, global hotkeys, and unrelated network entry points. Runtime-owned per-session paths are not embedded in this template.
- [ ] Implement `scripts/test-managed-everything-installer.ps1` with a non-mutating `Source` mode covering all Helper/template/build contracts and proving production fault injection is disabled. Task 3 owns the mutating VM modes and consumes the instrumented checkpoint build.
- [ ] Add a resource-only mode to `scripts/test-everything-package.ps1` that validates the committed `src-tauri/resources/everything` executable/lock/license without requiring gitignored download/staging artifacts; preserve its existing default full-package behavior.

**Verify:**

- `cargo test --manifest-path src-tauri/Cargo.toml --bin uipilot-everything-installer`
- `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/build-managed-everything-installer.ps1 -Configuration Release`
- `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/build-managed-everything-installer.ps1 -Configuration Release -Mode VmInstrumentedSetup`
- `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-installer.ps1 -Mode Source`
- `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-everything-package.ps1 -ResourceOnly`
- `npm run tauri -- build --bundles nsis`
- Inspect the generated Setup with the source test's bundle assertions; do not install it on the developer machine in this task.

---

### Task 2: Add the PID-Bound Runtime Supervisor and Route Production Search Through It

**Files:**

- Create: `src-tauri/src/file_search/everything_runtime.rs`
- Create: `src-tauri/src/file_search/windows/everything_process.rs`
- Modify: `src-tauri/src/file_search/windows/mod.rs`
- Modify: `src-tauri/src/file_search/mod.rs`
- Modify: `src-tauri/src/file_search/everything.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lifecycle.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `spikes/everything-ipc/src/client.rs`
- Modify: `spikes/everything-ipc/src/window.rs`
- Modify: `spikes/everything-ipc/src/lib.rs`
- Modify: `spikes/everything-ipc/Cargo.toml`
- Modify: `spikes/everything-ipc/tests/live_query.rs`
- Modify: `spikes/everything-ipc/tests/query_semantics.rs`
- Modify: `src/find-core.ts`
- Modify: `src/find-core.test.ts`
- Modify: `src/launcher-core.ts`
- Modify: `src/launcher.test.tsx`

**Dependencies:** Task 1 must provide the bundled client, lock, fixed INI template, private Service identity, and installer-owned security contract. Follow design sections `总体架构`, `子系统 2：运行时客户端 Supervisor`, `状态与错误行为`, and the ResultRegistry replacement contract under `已确认决策`.

**Distinct test coverage:**

- IPC tests reject a same-class window owned by the wrong PID, accept the expected PID, repeat ownership validation on reconnect, expose database-loaded state, and preserve current Query2 reply-source, flags, sort, deadline, and overload protections.
- Supervisor state-machine tests cover non-blocking startup, one owner per session, exact managed-orphan handling, invalid resource/manifest/process rejection, an empty database becoming Ready, fixed probe mismatch, three bounded retries, terminal Unavailable, and idempotent shutdown. If generation N fails after generation N+1 is published, invalidating N must leave N+1 Ready and cached; invalidating the current generation must retire it and begin only the allowed bounded restart.
- Windows process tests prove Job creation precedes process creation and that `PROC_THREAD_ATTRIBUTE_JOB_LIST` with `CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT` establishes ownership before the process can run. Failure injection at Job, attribute-list, CreateProcess, resume, and wait stages must leave no unowned child.
- Backend tests prove production search never calls `EverythingClient::connect("")`, searches only with a Supervisor-provided owned client, never caches a client while not Ready, preserves current ResultRegistry admission/retirement behavior, and accepts a legitimate zero-result index.
- Lifecycle tests cover tray quit, system session end, normal `RunEvent::Exit`, duplicate shutdown calls, and main/find window hiding without terminating the client.
- Frontend tests distinguish a stable “文件搜索正在准备” state from a stable unavailable state without rendering backend diagnostic details.

**Implementation points:**

- [ ] Extend the IPC crate with `connect_owned("UiPilot", expected_pid, timeout)` and a database-loaded probe. Resolve the named window class, obtain its PID with `GetWindowThreadProcessId`, and reject before worker creation if it differs. Keep the non-Windows stubs deterministic.
- [ ] Introduce a narrow runtime-client provider seam so `EverythingSearchState` can request a generation-tagged ready-client lease without owning process lifecycle. Move cached-client ownership into the Supervisor and preserve the current error-eviction classification through `invalidate_if_current(generation)`, so a late failure from an old lease cannot evict its replacement. Keep the checked revision allocator, literal-query encoding, category filters, and item authentication; remove the default-instance connect and the current probe that treats `total == 0` as unavailable.
- [ ] Implement the Supervisor as an internally synchronized state machine. Production setup schedules startup on a background worker and returns immediately; search reads a state snapshot and never waits on the 60-second readiness deadline.
- [ ] Resolve `Everything.exe`, lock, and template only through Tauri's resource directory. Verify canonical paths, SHA-256, architecture/version, and Authenticode publisher before process inspection or launch; never search `PATH`, registry, Program Files, or user Everything locations.
- [ ] Acquire `Local\UiPilotEverythingRuntime` in the current session. Enumerate only `EVERYTHING_TASKBAR_NOTIFICATION_(UiPilot)`. Gracefully stop and bounded-wait for an orphan only when its canonical executable, hash, and signature match the managed resource; an identity mismatch is terminal Unavailable.
- [ ] Parse the current session ID as `u32`, create the per-user/per-session root, and atomically regenerate the managed INI from the read-only template on every start. Preserve only the DB. Force the private instance, complete Service pipe, INI/DB paths, `-startup`, hidden UI, disabled update/network settings, and current non-elevated user token.
- [ ] Create the kill-on-close Job first; initialize the attribute list; create the client suspended and atomically in that Job; retain process/thread/Job handles; then resume. Never use post-create `AssignProcessToJobObject`. Release partially initialized resources in strict reverse order.
- [ ] Within the asynchronous 60-second budget, require the private Service to be Running, PID-bound IPC to connect, the database-loaded probe to succeed, and a Query2 capability probe to return request flags `0x155` and date-modified-descending sort. Record `total` only as a diagnostic scalar and allow zero.
- [ ] Monitor the exact child handle. On unexpected exit, clear the IPC client and increment only the internal generation; restart at most three times with bounded backoff. Missing/stopped Service, pipe mismatch, invalid resources, foreign process/window, or exhausted retries becomes terminal Unavailable with a stable reason code and no in-app elevation/repair attempt.
- [ ] Construct and manage one Supervisor before `EverythingSearchState`; start it during production setup. Wire its idempotent shutdown into `prepare_application_shutdown` and keep `RunEvent::Exit` as a final fallback so tray quit, session end, and ordinary exit all reject queries before releasing the Job. Never stop the Service.
- [ ] Add a public command-error code for Starting and retain the existing unavailable code for terminal failure. Map both to fixed Chinese UI text in the dedicated find window and launcher compatibility path; keep the existing stable status area in both themes and never forward raw runtime reasons.
- [ ] Add only the required `windows` crate feature flags for Job Objects, process/token/session inspection, Service status, signature verification, and window PID lookup. Keep Windows-only code behind `cfg(windows)` and preserve compilation of non-Windows IPC stubs.

**Verify:**

- `cargo test --manifest-path spikes/everything-ipc/Cargo.toml`
- `cargo test --manifest-path src-tauri/Cargo.toml file_search::everything_runtime -- --nocapture`
- `cargo test --manifest-path src-tauri/Cargo.toml file_search::everything -- --nocapture`
- `cargo test --manifest-path src-tauri/Cargo.toml commands::tests::search_files -- --nocapture`
- `npm test -- src/find-core.test.ts src/launcher.test.tsx`

---

### Task 3: Build the Automated Release Gate and Execute the Windows VM Matrix

**Files:**

- Create: `scripts/test-managed-everything-runtime.ps1`
- Create: `scripts/test-managed-everything-release.ps1`
- Create: `docs/testing/managed-everything-vm-matrix.md`
- Modify: `scripts/test-managed-everything-installer.ps1`
- Modify: `.gitignore`

**Dependencies:** Tasks 1 and 2 must be independently reviewed and green. Follow design sections `子系统 3：验证与发布门禁` and `实施拆分`. A freshly built NSIS Setup and disposable Windows VMs with administrator, standard-user, second-user, filtered-admin, elevated-admin, console, and RDP test identities are required for mutating rows.

**Distinct test coverage:**

- The release runner fails closed unless package verification, NSIS source gate, IPC tests, Rust backend tests, frontend tests, production build, and NSIS build all pass in order.
- Installed-runtime probes validate the private Service identity/ImagePath/arguments, manifest and ProgramData ACLs, resource identity, client command line, session-specific INI/DB, owned PID/window, Job membership, readiness, retry budget, shutdown cleanup, and coexistence with a default Everything instance.
- Security probes read the live named-pipe DACL with `GetSecurityInfo`, normalize it against the manifest SDDL, and run `AccessCheck` plus actual connect/create-instance probes for every positive and negative token listed in the approved design.
- Installer fault injection covers cancellation before Install Section; writable `$INSTDIR` source replacement; bootstrap-root/journal rename boundaries; Helper/parent Setup/system termination at every Service phase; rollback failure; explicit-uninstall interruption before each referenced-directory and manifest-last deletion; foreign Service/root/manifest/mutex/event conflicts; upgrade SID preservation; and downgrade/repair behavior.
- Installer concurrency tests cover standard-user mutex/activity-event squatting and descriptor changes, two simultaneous Setups, Setup versus true uninstall, abandoned ownership, parent Setup death while the old Helper activity lease remains live, and recovery by a different elevated administrator with distinct operation/recovered-transaction IDs.
- Multi-session tests prove console and RDP sessions for the authorized SID use different client processes, INIs, and DBs while sharing only the private Service.

**Implementation points:**

- [ ] Make `test-managed-everything-runtime.ps1` read-only by default and emit redacted structured evidence. Put process termination, Service mutation, token creation/logon, pipe creation, installer invocation, failure injection, and uninstall behind explicit named modes with elevation/disposable-VM guards.
- [ ] Make `test-managed-everything-release.ps1` the single local automated gate. In order it runs `cargo test --manifest-path src-tauri/Cargo.toml --bin uipilot-everything-installer`, the release Helper build plus PE `asInvoker`/architecture/no-fault-path inspection, installer Source gate, Everything resource-only package verification, complete IPC tests, complete Rust host tests, frontend tests/build, and production NSIS build. It must stop at the first failure, preserve individual exit codes, print the failing stage, and place generated Setup hashes and redacted outputs under the ignored artifacts root.
- [ ] Extend the installer test script with VM-only transaction modes that invoke only Task 1's independently named VM Instrumented Setup, target each named deterministic checkpoint through that Setup, and require elevation plus a disposable-VM marker. Reuse Source-mode canonicalization and ownership assertions so tests cannot broaden cleanup scope; never invoke the Helper directly or add fault controls to the production Helper/Setup.
- [ ] Write `docs/testing/managed-everything-vm-matrix.md` as an operator checklist mapping all 13 approved design rows to prerequisites, exact script mode/command, expected stable outcome, evidence filename, cleanup, and pass/fail/not-run status. Include both manual Setup replacement and Tauri `/UPDATE` paths.
- [ ] Ignore `artifacts/managed-everything/` explicitly. Evidence must redact machine/user/SID/path data and must never include search text or result paths.
- [ ] After explicit user approval to mutate disposable VMs, execute every available row. A release is blocked by any failure or required row marked not-run; local automated success alone is not sufficient to claim installation readiness.

**Verify:**

- `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-release.ps1 -Mode Local`
- `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-installer.ps1 -Mode Source`
- `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-runtime.ps1 -Mode Source`
- `npm run tauri -- build --bundles nsis`
- After explicit approval and only in disposable VMs: run the exact per-row commands recorded in `docs/testing/managed-everything-vm-matrix.md`, then confirm all required rows are `PASS` and the installed `/find` smoke test returns results for the intended desktop user.

## Completion Criteria

- A clean-machine per-machine Setup installs a verified private Everything Service without touching any user-owned Everything installation.
- The desktop user can launch `UiPilot_tools.exe` normally and use `/find` after asynchronous readiness; no manual Everything startup is required.
- Normal exit, tray exit, logoff, restart, crash, and forced termination leave no UiPilot-owned client process; the private Service remains installed/running until true uninstall.
- Upgrade, repair, rollback, uninstall cancellation, true uninstall, multi-session, pipe ACL, and conflicting-state behaviors match the approved design and have recorded VM evidence.
- All automated gates pass, all required VM rows pass, and the final independent review reports no actionable issue.
