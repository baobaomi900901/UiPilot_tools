# Public Plugin Host-Managed HTTPS Network Implementation Plan

**Goal:** Add the approved Windows-only Host-managed HTTPS capability for public
plugin Runtime command contexts at Host `0.3.2`, while keeping WebView CSP closed
and keeping translation/provider logic outside this repository change.

**Architecture:** The approved
[`Public Plugin Host-Managed HTTPS Network Design`](../specs/2026-08-27-public-plugin-host-https-network-design.md)
is authoritative. Runtime calls one deep `PluginHttpsBroker`; a shared
`PluginNetworkAuthorityGate` linearizes admission, lifecycle invalidation, and
terminal delivery; `NativeHttpsTransport` owns Windows DNS/TLS/HTTP details; the
durable plugin state owns exact-host consent and revocation.

## Technology And Hard Constraints

- Rust 1.96, Tauri 2.11, TypeScript 7, React 19, Vitest 4, schemars 1.2,
  Ajv 8, and the selected async HTTP stack pinned in `Cargo.lock`.
- Before broad implementation, prove that the selected production transport can
  pin validated addresses while retaining hostname TLS validation, disable
  proxies and pooling, enforce parser-level response-header limits, and stop
  delivering cancelled work. Do not weaken the approved policy to fit a client.
- Windows only; API v1 stays unchanged and Host becomes `0.3.2`.
- Follow design sections 7.2 and 13.1 for the single authority linearization
  point and lock order. Hold no Host lock during DNS or network I/O.
- Preserve the existing durable `NotCommitted` / `Committed` / `Unknown`
  transaction model. Do not redesign installation persistence.
- Do not alter WebView CSP, add general `fetch`, contact public services in
  tests, store credentials, or modify any example/third-party plugin.
- `src-tauri/src/public_plugins/activation.rs` already contains user changes.
  Read and preserve them, and stage only task-owned hunks. Preserve every other
  pre-existing dirty or untracked file.
- Automated work never controls keyboard or pointer input. Manual acceptance
  pauses for the user and uses an operator-controlled endpoint/package outside
  committed plugin examples.

## Shared Contract

Use the exact Manifest, Runtime DTO, error, address, limit, redirect,
cancellation, logging, and state contracts from design sections 4-14. In
particular, the public property is optional `api.network`, HTTP statuses resolve,
policy/transport errors reject with the fixed `{ code }` mapping, and one context
is limited to eight attempts and two concurrent calls while the Host permits
sixteen concurrent calls.

## Global Execution Rules

- Every task follows focused TDD: add failing tests, confirm the intended
  failure, implement the minimum approved behavior, rerun focused tests, and
  commit atomically.
- Every task receives specification-compliance and code-quality review before a
  dependent task begins. Review fixes use separate focused commits when needed.
- Generated Schema and validator files are regenerated through repository
  scripts, never edited independently.
- Commit only task-owned paths/hunks; never absorb the existing workspace
  changes.
- Use approved-spec commit `c9ec2e0` as `IMPLEMENTATION_START_SHA` for the final
  committed-range checks. The range therefore includes the plan commit and all
  implementation commits.
- Dependency order:
  `Task 1 -> Task 3 -> Task 2 -> Task 4 -> Task 5 -> Task 6 -> Task 7 -> Task 8`.

### Task 1: Host Version And Manifest Validation Contract

**Files:** `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`,
`src-tauri/Cargo.lock`, `src-tauri/tauri.conf.json`, `src-tauri/src/lib.rs`,
`src-tauri/src/public_plugins/manifest.rs`,
`src-tauri/src/public_plugins/tests.rs`,
`docs/plugin-sdk/uipilot-plugin-v1.schema.json`,
`packages/plugin-cli/schema/uipilot-plugin-v1.schema.json`,
`packages/plugin-cli/src/generated/manifest-validator.mjs`,
`packages/plugin-cli/src/manifest.ts`, `packages/plugin-cli/src/validate.ts`,
`packages/plugin-cli/tests/manifest.test.ts`,
`packages/plugin-cli/tests/validate.test.ts`.

**Dependencies:** Approved design sections 4, 17, and 18.1.

- [ ] Upgrade every Host version source to `0.3.2` while retaining API v1.
- [ ] Add strict optional `network.httpsHosts`, exact cross-field permission
  matching, canonical ordering, hostname grammar, limits, and Windows/version
  validation.
- [ ] Synchronize Rust, canonical Schema, bundled CLI Schema, generated
  validator, CLI types, and semantic validation without adding network code to
  the CLI.

**Distinct test coverage:** Accept old manifests plus legal one/eight-host
Windows manifests; reject every malformed, duplicate, over-limit,
permission/field mismatch, macOS, and `minimumHostVersion` case enumerated in
section 18.1; prove Rust and CLI fixtures agree and the packed CLI remains
network-incapable.

**Verify:**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml public_plugins::manifest
cargo test --manifest-path src-tauri/Cargo.toml plugin_network_manifest_
cargo run --manifest-path src-tauri/Cargo.toml --bin generate_public_plugin_schema -- --check
npm.cmd test --workspace @uipilot/plugin-cli -- manifest.test.ts validate.test.ts
npm.cmd run verify --workspace @uipilot/plugin-cli
```

### Task 2: Durable Exact-Host Authorization

**Files:** `src-tauri/src/public_plugins/state.rs`,
`src-tauri/src/public_plugins/state_tests.rs`,
`src-tauri/src/public_plugins/activation.rs`,
`src-tauri/src/public_plugins.rs`, `src-tauri/src/commands.rs`.

**Dependencies:** Task 3; design sections 5, 15, and 18.2.

- [ ] Store the exact canonical host grant with the active package generation,
  add fail-closed loading, and expose prepare/inventory authority summaries.
- [ ] Implement atomic install/update consent, automatic host narrowing,
  durable revoke, and regrant from the current active package.
- [ ] Derive generation-bound authorization snapshots for later gate admission
  without changing the existing install transaction protocol.

**Distinct test coverage:** Fresh consent persists atomically; cancelling an
added-host update, or a verified `NotCommitted` outcome whose old digest/state
summary still matches, preserves the old version and grant. `Unknown` or
post-durable-commit publication failure publishes a closed authorization
snapshot pending Task 5's request cancellation integration, makes the service
terminal, and recovers from durable state only after restart.
Removed-host update narrows without consent; revoke survives restart/update;
corrupt legacy state fails closed. Assert revision order and exact terminal
grant state for each case.

**Verify:**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml public_plugins::state_tests
cargo test --manifest-path src-tauri/Cargo.toml plugin_network_grant_
```

### Task 3: Native HTTPS Transport And Address Policy

**Files:** `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`,
`src-tauri/src/public_plugins.rs`, create
`src-tauri/src/public_plugins/network/mod.rs`,
`src-tauri/src/public_plugins/network/address_policy.rs`,
`src-tauri/src/public_plugins/network/transport.rs`.

**Dependencies:** Task 1; design sections 8.3, 9, 10, 15, and 18.4.

- [ ] Add the production `NativeHttpsTransport` plus deterministic test seam,
  fixed IPv4/IPv6 deny tables, mapped-address normalization, and bounded DNS
  executor.
- [ ] Pin the complete validated address set while using the original hostname
  for Windows trust/SNI; require TLS 1.2+, identity encoding, no proxy, no
  cookies, and no cross-call/plugin connection reuse.
- [ ] Enforce response field-count and byte limits in the protocol parser before
  broker buffering; stream body bytes and honor the one total deadline and
  cancellation token.

**Distinct test coverage:** Every CIDR edge, empty/mixed answers, mapped IPv6,
hostname/trust/expiry failures, HTTP downgrade, ignored proxy environment, and
fresh connections. A local TLS harness proves address pinning with hostname TLS
validation and rejects one oversized field, too many fields, and progressive
header overflow at parser level. Repeated cancelled/abandoned DNS lookups remain
inside the fixed worker and queue bounds until the underlying lookup exits.

**Verify:**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml native_https_transport_
cargo test --manifest-path src-tauri/Cargo.toml network_address_policy_
```

### Task 4: Broker Policy And Request Registry

**Files:** create `src-tauri/src/public_plugins/network/broker.rs`,
`src-tauri/src/public_plugins/network/registry.rs`; modify
`src-tauri/src/public_plugins/network/mod.rs`.

**Dependencies:** Tasks 2 and 3; design sections 6, 7.2, 8, 10-12, 14, 15,
18.3, and 18.5.

- [ ] Implement `PluginHttpsBroker` as the sole policy/execution seam for URL,
  methods, bodies, headers, redirects, response filtering/UTF-8, limits, fixed
  errors, and redacted logs.
- [ ] Implement generation/context/call identities, checked attempt sequences,
  two-level concurrency reservations, cancellation tokens, and exactly-once
  terminal compare-and-set in `PluginNetworkRequestRegistry`.
- [ ] Revalidate exact authority and DNS on each same-host redirect; reject every
  cross-host redirect before forwarding headers or body.

**Distinct test coverage:** Exact bytes for GET and all POST body forms; every
limit and protected-header rule; 4xx/5xx resolve; invalid encoding/UTF-8 fails;
three-hop redirect succeeds and the fourth fails; cross-host redirect observes
no forwarded secret/body. Counter exhaustion never wraps; third per-context and
seventeenth Host-wide concurrent calls reject without slot leaks; response and
cancel each win terminal CAS in separate ordered tests.

**Verify:**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml plugin_https_broker_
cargo test --manifest-path src-tauri/Cargo.toml plugin_network_registry_
```

### Task 5: Authority Gate And Lifecycle Cancellation

**Files:** create
`src-tauri/src/public_plugins/network/authority_gate.rs`; modify
`src-tauri/src/public_plugins/network/mod.rs`,
`src-tauri/src/public_plugins/activation.rs`,
`src-tauri/src/public_plugins/runtime.rs`,
`src-tauri/src/public_plugins/scheduler.rs`,
`src-tauri/src/public_plugins/owner_cleanup.rs`,
`src-tauri/src/public_plugins/delayed_messages.rs`,
`src-tauri/src/public_plugins.rs`.

**Dependencies:** Task 4; design sections 7.2, 13, 15, and 18.5.

- [ ] Make `PluginNetworkAuthorityGate` the common linearization point for
  admission, lifecycle invalidation, cancellation, and delivery recheck using
  the fixed lock order.
- [ ] Cancel exact calls on replacement, context completion/failure/timeout,
  revoke, disable, fault-disable, update, uninstall, recovery, WebView teardown,
  and Host shutdown.
- [ ] Release concurrency slots exactly once and prevent stale identities from
  cancelling or delivering into newer generations/contexts.

**Distinct test coverage:** Barrier tests prove an authority transition cannot
  slip between admission validation and registry insertion; revoke/replacement
  between transport completion and delivery prevents resolution; old
  generation cancellation cannot affect a newer call with reused local
  sequence. A parameterized test starts with one registered blocked request for
  each replacement, completion, failure, timeout, revoke, disable,
  fault-disable, update, uninstall, recovery, uncertain install outcome, and
  post-commit publication-failure transition; after the transition its token
  is cancelled, its Broker future returns the internal `expiredRequest` wire
  code, its slots release exactly once, and the management path does not wait
  for the ten-second deadline. Separate tests exercise the real WebView teardown
  and Host shutdown entries with the same terminal guarantees. A blocking
  adapter proves no authority, scheduler, state, or registry lock remains held
  while network I/O is pending.

**Verify:**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml plugin_network_authority_
cargo test --manifest-path src-tauri/Cargo.toml plugin_network_lifecycle_
```

### Task 6: Runtime API Command And Capability Boundary

**Files:** `src-tauri/src/public_plugins/runtime.rs`,
`src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`,
`src-tauri/capabilities/plugin-runtime.json`,
`src-tauri/permissions/autogenerated/plugin_network_request.toml`,
`docs/plugin-sdk/uipilot-plugin-api-v1.d.ts`, create
`docs/plugin-sdk/tests/network-api-contract.ts`, create
`src/plugin-network-bootstrap.test.ts`.

**Dependencies:** Task 5; design sections 6, 7, 11-12, 15, 17, and 18.6.

- [ ] Add one async runtime-only Tauri command that derives plugin ID and
  generation from the caller label, strictly parses the submitted
  `PluginRequestContext`, and validates label agreement plus scheduler
  currentness before invoking the broker.
- [ ] Expose frozen optional `api.network.request` only to declaring command
  Runtimes; snapshot/freeze input and reject bad arity, unknown fields, or stale
  context before any transport work.
- [ ] Map the exact Host `{ code }` DTO to the nine fixed JavaScript Error names
  without private dependency, URL, body, header, or address detail.
- [ ] Synchronize the API v1 declaration and a non-plugin TypeScript compile
  fixture for GET, POST, all bodies, responses, and error narrowing.

**Distinct test coverage:** `main`, panel, window, and unrelated runtime labels
are rejected before protected state access; only `plugin-runtime-*` has the
request capability. Declared and undeclared bootstrap surfaces differ exactly;
malformed input fails closed; stale command completion receives no response;
every fixed code maps to the matching public Error name, including a cancelled
Broker future rejecting its JavaScript Promise as `ExpiredRequestError`.

**Verify:**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml plugin_network_runtime_
cargo test --manifest-path src-tauri/Cargo.toml plugin_network_command_
npm.cmd test -- src/plugin-network-bootstrap.test.ts
npm.cmd exec tsc -- --ignoreConfig --noEmit --strict docs/plugin-sdk/tests/network-api-contract.ts
```

### Task 7: Install Consent And Settings Network Controls

**Files:** `src/protocol.ts`, `src/protocol.test.ts`, `src/main.ts`,
`src/public-plugin-panel.tsx`, `src/launcher.test.tsx`, `src/styles.css`,
`src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`,
`src-tauri/capabilities/main.json`, create
`src-tauri/permissions/autogenerated/set_public_plugin_network_access.toml`.

**Dependencies:** Tasks 2 and 6; design sections 5.1-5.4, 16, and 18.6.

- [ ] Carry sorted exact hosts and added-host state through prepare/inventory
  parsers, and require explicit install/update confirmation for expanded
  authority.
- [ ] Show non-link host lists, installed grant state, revoke switch, and regrant
  confirmation; refresh from Host authority after every mutation.
- [ ] Keep pending controls disabled, use fixed local failures, and restore focus
  to the initiating control with the existing settings focus pattern.

**Distinct test coverage:** Fresh install and added-host update require consent;
cancel commits nothing; removed hosts do not re-prompt. Revoke and regrant show
the exact current host set, refresh authoritative inventory, retain focus on
success/failure, and never expose provider response or credential fields.

**Verify:**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml plugin_network_management_
npm.cmd test -- src/protocol.test.ts src/launcher.test.tsx
npm.cmd run build
```

### Task 8: Public Documentation And Focused Acceptance

**Files:** `docs/plugin-sdk/public-plugin-v1.md`,
`docs/plugin-sdk/public-plugin-developer-guide.md`.

**Dependencies:** Tasks 1-7; design sections 17, 19-21.

- [ ] Document exact Manifest consent, optional Runtime API, request/response and
  errors, security limits, Windows-only behavior, cancellation, and the fact
  that embedded test keys are inspectable and production secret consumption is
  deferred.
- [ ] Run the relevant Rust, Schema, CLI, SDK, frontend, formatting, and compile
  gates once; inspect the final diff for plugin/provider scope expansion.
- [ ] Pause and give the user section 19's manual steps for real-window and
  operator-controlled HTTPS acceptance. Do not automate input or contact a real
  provider.

**Distinct test coverage:** Documentation links resolve to the canonical Schema
and API declaration; no example plugin or provider fixture changed; the final
manual checklist covers success, denial, timeout/size, replacement, revoke,
update consent, disable, and uninstall terminal behavior.

**Verify:**

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml public_plugins
cargo test --manifest-path src-tauri/Cargo.toml plugin_network_
cargo run --manifest-path src-tauri/Cargo.toml --bin generate_public_plugin_schema -- --check
npm.cmd run verify --workspace @uipilot/plugin-cli
npm.cmd test -- src/protocol.test.ts src/plugin-network-bootstrap.test.ts src/launcher.test.tsx
npm.cmd run build
npm.cmd exec tsc -- --ignoreConfig --noEmit --strict docs/plugin-sdk/tests/network-api-contract.ts
git diff --check c9ec2e0..HEAD
git diff --name-only c9ec2e0..HEAD
```

## Final Checklist

- [ ] Design section 20 acceptance criteria are satisfied without plugin,
  provider, credential-storage, CSP, WebSocket, upload, or background-network
  work.
- [ ] Independent review reports no unresolved correctness/security findings.
- [ ] Existing user changes remain present and outside feature commits.
- [ ] Automated verification results and user-owned manual acceptance steps are
  reported with final commit SHAs; nothing is pushed unless explicitly asked.
