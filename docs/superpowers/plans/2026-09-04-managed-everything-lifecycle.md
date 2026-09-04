# UiPilot Managed Everything Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `subagent-driven-development` (recommended) or `executing-plans` to implement this plan task-by-task. Use `requesting-code-review` after each task and `verification-before-completion` before reporting it complete.

**Goal:** Make the installed Windows build start and own a private Everything client automatically, backed by a UiPilot-managed Everything Service, so `/find` works for the desktop user without a separate Everything installation.

**Architecture:** The elevated NSIS transaction owns the machine-wide Service executable, Service registration, pipe ACL, and protected manifest. A production-only Rust supervisor owns the per-user/per-session hidden client, Job Object, readiness state, and PID-bound IPC connection. Existing Query2 payload handling, ResultRegistry behavior, and authenticated file execution remain unchanged. A release gate combines deterministic source/unit checks with an explicit Windows VM matrix.

**Tech Stack:** Tauri CLI 2.11.4, NSIS, Everything 1.4.1.1032 x64 Stable, Rust 2021, `windows` 0.61.3, PowerShell 5.1+, React 19, TypeScript 7, Vitest 4.

**Source Spec:** [Approved managed Everything lifecycle design](../specs/2026-09-04-managed-everything-lifecycle-design.md)

## Global Constraints

- Windows x64 installed builds are in scope. Development startup behavior remains on the existing developer scripts unless a task explicitly says otherwise.
- Use only the private identities fixed by the design: instance `UiPilot`, Service `Everything (UiPilot)`, and pipe `\\.\PIPE\UiPilotEverythingService`.
- Never discover, connect to, modify, stop, or uninstall the user's default Everything instance, Service, configuration, database, or process.
- Keep the Service executable and `managed-runtime.json` beneath the protected `%ProgramData%\UiPilot_tools\everything` root. Keep client INI/DB beneath the current user's `%LOCALAPPDATA%\UiPilot_tools\everything\<session-id>` root.
- Treat the approved SDDL, desktop shell SID acquisition, signed-resource validation, manifest matching, and fail-closed ownership checks as security boundaries. Do not replace them with usernames, environment variables, active-console guesses, process names, or install-directory trust.
- Keep startup non-blocking. Everything readiness or failure must not delay the main window, tray, calculator, plugins, quick links, or settings.
- Keep ResultRegistry semantics unchanged: client restart alone does not invalidate published actions; the next admitted query retires the old set; a failed query leaves the set empty; execution still reauthenticates every path component.
- Keep private instance, pipe, INI, DB, process arguments, and runtime generation entirely Rust/installer-owned; the WebView must not submit or override them.
- Log stable reason codes only. Do not log searches, result paths, SID values, pipe names, private filesystem paths, or raw system errors to user-visible surfaces.
- Do not run installer, uninstaller, Service, ACL, token, or multi-user mutation tests on the developer machine without explicit user approval. Use disposable Windows VMs for the release matrix.
- The existing uncommitted `src-tauri/tauri.conf.json` `perMachine` change belongs to this feature. Task 1 may absorb that exact change; do not stage unrelated worktree changes.

## Core Contract Overview

- `managed-runtime.json` is the authoritative machine-owned record. It contains the fixed product identity plus version, architecture, SHA-256, Authenticode publisher, canonical Service executable, `desktop_user_sid`, and expanded SDDL. Installer repair, upgrade, rollback, and uninstall act only after both this manifest and live SCM/filesystem state match.
- `EverythingRuntimeSupervisor` exposes only `Starting`, `Ready`, `Unavailable(reason)`, and `Stopping`, plus a ready-client lease containing an internal generation and PID-bound client. Query failure may invalidate a lease only when its generation is still current. Process handles, Job Object, generation values, retry accounting, paths, and diagnostic details stay out of ResultRegistry and frontend contracts.
- `EverythingClient::connect_owned(instance, expected_pid, timeout)` accepts an IPC window only when `GetWindowThreadProcessId` returns the expected child PID. Every reconnect repeats this binding. Database-loaded and fixed Query2-capability probes are explicit readiness checks; `total == 0` is valid.
- Shutdown is idempotent: first reject new searches, then close the cached IPC client, gracefully stop and bounded-wait for the owned child, terminate the exact child only if needed, and finally close the Job handle. The machine Service remains running.

## Global Execution Rules

1. Implement Tasks 1–3 in order; each task ends in one atomic commit and a review gate before the dependent task starts.
2. Every task follows TDD: add focused failing tests, confirm the intended failure, implement the minimum contract, rerun the focused tests, and commit only green behavior. Do not commit skipped security assertions.
3. After each implementation commit, launch an independent review agent against that commit and the approved design. Resolve actionable findings and repeat independent review until it reports no actionable issue, as the user requested.
4. Keep the custom NSIS template byte-for-byte aligned with the Tauri CLI 2.11.4 upstream template outside the two approved behavior regions. Any future CLI upgrade must fail the source gate until the template is explicitly rebased.
5. Record Windows VM evidence outside Git under `artifacts/managed-everything/`; do not commit machine names, usernames, SIDs, local paths, or raw logs containing them.
6. If a required Windows token/session topology cannot be created, report the unexecuted matrix row; never substitute a weaker assertion and mark it passed.

---

### Task 1: Implement the Transactional NSIS Service Lifecycle

**Files:**

- Create: `src-tauri/windows/installer.nsi`
- Create: `src-tauri/windows/nsis-hooks.nsh`
- Create: `src-tauri/resources/everything/Everything-UiPilot.ini`
- Create: `scripts/test-managed-everything-installer.ps1`
- Modify: `src-tauri/tauri.conf.json`

**Dependencies:** None. Follow design sections `固定身份与路径`, `子系统 1：NSIS Service 生命周期`, and `子系统 3：验证与发布门禁`. This task consumes the reviewed Everything executable, license, and lock already under `src-tauri/resources/everything/`.

**Distinct test coverage:**

- Snapshot/source gate proves the custom template is derived from Tauri CLI 2.11.4 and differs only in the approved reinstall flow and uninstall gate ordering.
- Pure installer-helper tests cover canonical paths, strict manifest field matching, semantic SCM argument comparison, expanded SDDL construction/validation, same-session Explorer SID selection, upgrade SID preservation, and rejection of missing/foreign/ambiguous state.
- Transaction tests cover fresh install, auto-update, manual replacement, same-version repair, explicit uninstall, pre-commit cancellation, Service switch failures, rollback failures, `/UPDATE` compatibility, and cleanup restricted to the managed root.
- Source assertions prove the fixed instance, Service, complete pipe name, ProgramData layout, resource inputs, signature/hash/version checks, Service-running check, and protected manifest replacement are present.

**Implementation points:**

- [ ] Vendor the exact Tauri CLI 2.11.4 `installer.nsi`. Mark the two permitted custom regions and implement: all replacement/repair paths continue in-place without invoking the old uninstaller; explicit same-version uninstall invokes it without `/UPDATE` and exits; the uninstall app-running gate executes before `NSIS_HOOK_PREUNINSTALL`.
- [ ] Point `bundle.windows.nsis.template` and `installerHooks` at the new files, retain `installMode: "perMachine"`, and add `Everything-UiPilot.ini` to bundle resources.
- [ ] Make `PREINSTALL` read-only with respect to the live Service. Validate an existing protected manifest, canonical managed roots, Service name, SCM ImagePath, arguments, and old executable identity; capture complete rollback data. On upgrade/repair, preserve the manifest's `desktop_user_sid`.
- [ ] On fresh install, acquire the SID only through the same-session Explorer shell token using the Win32 chain in the design. Canonicalize it and use Windows security-descriptor APIs to build and inspect exactly the LocalSystem, Built-in Administrators, and desktop-user ACEs before any Service mutation.
- [ ] Make `POSTINSTALL` verify the bundled Everything version, lock hash, and Authenticode publisher before staging a versioned ProgramData copy with non-admin-writeable ACLs.
- [ ] Commit the Service switch in order: verify old ownership; stop/remove the old private Service only for an upgrade; install the new private Service with the fixed instance, full pipe, and expanded SDDL; validate canonical SCM state; start and confirm Running; atomically replace the manifest; then delete only unreferenced verified version directories.
- [ ] On any Service-switch failure, remove only a newly created matching Service, restore and start the old verified Service from captured rollback data, retain the old manifest and verified binaries, remove safe new staging, and fail Setup. If rollback also fails, retain all verified recovery material and emit a stable diagnostic code.
- [ ] On true uninstall, perform no external mutation until the customized app-running gate succeeds. `PREUNINSTALL` validates ownership and stops/removes only the private Service; `POSTUNINSTALL` deletes the manifest/version root only after the Service is confirmed absent. `/UPDATE` uninstall invocations leave all managed Service state untouched.
- [ ] Generate the INI template with the fixed private instance/pipe contract and disable tray UI, updates, self-update, HTTP, ETP, FTP, global hotkeys, and unrelated network entry points. Runtime-owned per-session paths are not embedded in this template.
- [ ] Implement `scripts/test-managed-everything-installer.ps1` with a default non-mutating `Source` mode. Put mutating transaction probes behind explicit modes that require elevation and a disposable-test marker.

**Verify:**

- `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-installer.ps1 -Mode Source`
- `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-everything-package.ps1`
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
- Installer fault injection covers cancellation before Install Section, failures after stopping old Service, after new Service installation, before/after start, rollback failure, explicit-uninstall cancellation, foreign Service/root/manifest conflicts, upgrade SID preservation, and downgrade/repair behavior.
- Multi-session tests prove console and RDP sessions for the authorized SID use different client processes, INIs, and DBs while sharing only the private Service.

**Implementation points:**

- [ ] Make `test-managed-everything-runtime.ps1` read-only by default and emit redacted structured evidence. Put process termination, Service mutation, token creation/logon, pipe creation, installer invocation, failure injection, and uninstall behind explicit named modes with elevation/disposable-VM guards.
- [ ] Make `test-managed-everything-release.ps1` the single local automated gate. It must stop at the first failure, preserve individual exit codes, print the failing stage, and place generated Setup hashes and redacted outputs under the ignored artifacts root.
- [ ] Extend the installer test script with the VM-only transaction and fault-injection entry points required by the matrix; reuse the same canonicalization and ownership assertions as its Source mode so tests cannot silently broaden cleanup scope.
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
