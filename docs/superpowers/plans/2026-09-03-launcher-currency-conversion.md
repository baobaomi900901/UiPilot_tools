# Launcher Currency Conversion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert ISO currency amounts with Frankfurter's latest published reference rates, one-hour fresh caching, and clearly labeled seven-day outage fallback.

**Architecture:** Extend the shared router from [Launcher Date Quick Evaluation And Shared Foundation](2026-09-03-launcher-date-quick-evaluation.md) with a synchronous currency intent parser and an asynchronous host-owned resolver. A replaceable provider adapter uses a fixed-origin bounded HTTPS transport; a disposable atomic cache stores only pair/rate/date/fetch metadata. The approved [Launcher Quick Evaluation Design](../specs/2026-09-03-launcher-quick-evaluation-design.md) is authoritative for provider, cache, security, ownership, UI, and failure contracts.

**Tech Stack:** Rust 1.96/Tauri 2, `rust_decimal` 1.43.0, existing Hyper 1.10.1/native-tls 0.2.14/Tokio 1.53.0/Serde, existing `AtomicPaths`, TypeScript 7, React 19, Lucide React, Vitest, Rust unit and deterministic local-TLS tests.

## Global Constraints

- The date/shared-foundation plan must be complete first. Unit and city-time plans may be present but are not semantic dependencies.
- Follow design sections `Currency Conversion`, `Launcher Result And Wire Contracts`, `Data Flow`, and `Failure And Safety Rules` exactly.
- Provider origin is fixed to `https://api.frankfurter.dev`; no user URL, API key, provider setting, redirect, arbitrary header, or public-plugin broker authority.
- GET only, two validated ASCII currency path segments, TLS hostname validation, 2-second end-to-end network deadline, and 64 KiB response-body limit.
- Currency uses decimal arithmetic and at most three fractional digits with trailing-zero and negative-zero normalization.
- Cache fresh age is at most one hour; stale fallback is over one hour through seven days; anything older is removed and never used.
- Persist only pair, canonical decimal rate, effective date, and UTC fetch timestamp. Never persist amount, raw query, result text, invocation ID, or application-search history.
- Recognized currency work starts current-query ownership before awaiting network; stale completion may refresh cache but never publish, copy, hide, or schedule work for newer input.
- Do not add live-network calls to the default test suite or expose raw provider failures.

## Global Execution Rules

- Dependency order is `Task 1 -> Task 2 -> Task 3 -> Task 4 -> Task 5`.
- Every task follows focused TDD and produces one atomic task-only commit. Review fixes use separate commits.
- Each task receives specification-compliance and code-quality review before its dependent task begins when available.
- Use injected clocks, transports, provider outcomes, and temporary cache directories for deterministic tests.
- Preserve pre-existing worktree changes. Do not launch/control the real GUI or run the manual Frankfurter smoke check without telling the user first.

---

### Task 1: Currency Intent, Decimal Parsing, Provider DTO, And Formatting

**Files:** Modify `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/src/quick_evaluation/mod.rs`; create `src-tauri/src/quick_evaluation/currency/mod.rs`, `src-tauri/src/quick_evaluation/currency/provider.rs`.

**Dependencies:** Shared foundation; design sections `Currency Conversion / User Grammar`, `Provider Choice And Boundary`, and `Decimal And Display Semantics`.

- [ ] Add exact compatible `rust_decimal = "1.43.0"` dependency and its lockfile resolution; parse the shared canonical number string into `Decimal` without changing accepted lexical syntax.
- [ ] Implement the currency recognizer as a pure module entry point. Require number, three ASCII-letter source, ASCII `to`, and three ASCII-letter target; uppercase codes for internal identity. Leave global dispatcher registration to Task 5 so async resolution and backend publication land together.
- [ ] Return `NoMatch` for incomplete forms, typed `Invalid` for a complete malformed code shape, and `Ready(CurrencyIntent)` for any syntactically valid three-letter pair. Provider support is resolved only after I/O in Task 3. Currency names and symbols remain unowned.
- [ ] Define provider-neutral request/result/error DTOs: amount and rate are decimals; codes are validated canonical strings; successful provider data includes exact base/quote and effective ISO date; errors distinguish unsupported code from unavailable transport/data.
- [ ] Validate and deserialize the Frankfurter v2 single-pair response without performing I/O in the parser; mismatched codes, invalid dates, nonpositive/nondecimal rates, and unknown required shape are rejected.
- [ ] Short-circuit same-code conversion at rate one. For all success paths, multiply with checked decimal arithmetic, round to at most three fractional digits, remove trailing zeroes/negative zero, and build approved title/subtitle/copy text.

**Distinct test coverage:** lowercase/mixed ISO input; signs, leading decimals, valid/invalid commas; incomplete shapes and currency-symbol fallthrough; same-code no-provider intent; exact three-decimal rounding and trailing-zero removal; provider JSON success; mismatched pair/date/rate/shape rejection; exact title, explanatory rate subtitle, and copy text.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml quick_evaluation::currency::provider`; `cargo test --manifest-path src-tauri/Cargo.toml quick_evaluation::currency::tests`

### Task 2: Disposable Atomic Exchange-Rate Cache

**Files:** Create `src-tauri/src/quick_evaluation/currency/cache.rs`, `src-tauri/src/quick_evaluation/currency/cache_tests.rs`; modify `src-tauri/src/quick_evaluation/currency/mod.rs`.

**Dependencies:** Task 1; design section `Currency Conversion / Cache Contract`.

- [ ] Define `exchange-rate-cache.json` as a versioned JSON cache with canonical pair, decimal-string rate, effective date, and UTC fetch time only; validate every field and enforce at most 256 entries.
- [ ] Canonicalize a pair independently of direction, storing one rate orientation and inverting with checked decimal arithmetic for reverse lookup.
- [ ] Classify lookup at an injected UTC instant as fresh, stale-fallback candidate, or expired. Fresh returns immediately; stale remains available only to the service's failure path; expired is removed.
- [ ] Load current then backup through `AtomicPaths`/`read_optional`; unknown schema, malformed entries, invalid dates/rates/timestamps, or two unusable files degrade to an empty in-memory cache.
- [ ] Persist validated snapshots with `commit_with_backup`, passing only previously validated current bytes as backup input. Evict by oldest fetch timestamp and prune expired entries on load and write without recording a separate last-access field.
- [ ] Treat read/write failure as cache unavailability rather than conversion failure; successful network data remains usable in memory even when persistence fails.

**Distinct test coverage:** missing/current/backup load order; corrupt current with valid backup; both corrupt; unknown schema; exact one-hour and seven-day boundaries; future/impossible timestamp rejection; reverse lookup/inversion; 256-entry oldest-fetch eviction; expired pruning; serialized data contains none of amount/raw query/result/invocation fields; atomic write failure preserves prior valid current/backup behavior.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml quick_evaluation::currency::cache_tests`

### Task 3: Fixed-Origin Frankfurter HTTPS Transport

**Files:** Create `src-tauri/src/quick_evaluation/currency/transport.rs`, `src-tauri/src/quick_evaluation/currency/transport_tests.rs`; modify `src-tauri/src/quick_evaluation/currency/provider.rs`.

**Dependencies:** Task 1; design sections `Currency Conversion / Provider Choice And Boundary`, `Currency Failure Behavior`, and `Failure And Safety Rules`.

- [ ] Define a provider transport trait returning bounded status/headers/body data so provider and service tests never require real network access.
- [ ] Implement the production transport with existing Hyper HTTP/1, Tokio TCP, and native-tls primitives. Build only `/v2/rate/{BASE}/{QUOTE}` after exact ASCII validation; set the fixed Host and JSON Accept headers.
- [ ] Resolve/connect only the fixed provider hostname, validate TLS for that hostname, enforce one end-to-end two-second deadline across DNS/connect/TLS/send/read, and stop body accumulation above 64 KiB.
- [ ] Do not implement redirect following. Map 200 to strict provider parsing, documented unsupported-code response to the typed unsupported error, and 3xx/429/other non-success, DNS, TLS, timeout, I/O, and oversized bodies to unavailable/data errors without leaking bodies.
- [ ] Reuse existing deterministic local TLS-test dependencies (`rcgen`, `tokio-rustls`) but do not make the public-plugin network transport public or route requests through its authority gate.

**Distinct test coverage:** exact method/path/Host/Accept request; lowercase or injected path characters cannot reach transport; trusted local TLS success; hostname/certificate failure; connection failure; deadline during connect/headers/body; body exactly at and above 64 KiB; 3xx proves no second request; 422 unsupported; 429/5xx unavailable; malformed 200 body rejected; no raw body in errors.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml quick_evaluation::currency::transport_tests`

### Task 4: Exchange-Rate Service, Coalescing, And Fallback Order

**Files:** Create `src-tauri/src/quick_evaluation/currency/service.rs`, `src-tauri/src/quick_evaluation/currency/service_tests.rs`; modify `src-tauri/src/quick_evaluation/currency/mod.rs`, `src-tauri/src/quick_evaluation/currency/cache.rs`.

**Dependencies:** Tasks 1 through 3; design sections `Currency Conversion / Cache Contract`, `Currency Failure Behavior`, and `Data Flow`.

- [ ] Implement an `ExchangeRateService` owning the provider, in-memory cache, atomic persistence path, and an in-flight map keyed by canonical unordered pair.
- [ ] On resolve, return same-code locally, then fresh direct/reverse cache, otherwise join or start exactly one provider task. The spawned provider task owns completion so cancellation of the first caller cannot strand later waiters.
- [ ] Broadcast one canonical validated result to direct and inverse waiters, update memory, and attempt serialized atomic persistence. Persistence failure must not turn a valid conversion into an error.
- [ ] On timeout/unavailable/data failure, return an eligible direct/reverse stale entry with explicit `缓存`, effective date, and fetch time; otherwise return `汇率暂不可用`. Unsupported-code results bypass stale fallback and return the typed invalid-code row.
- [ ] Remove every in-flight entry and settle every waiter on all terminal paths, including panic/join failure converted to unavailable. Do not hold cache/in-flight locks during DNS, network, TLS, disk I/O, or waiter notification.
- [ ] Permit a stale launcher caller's successful provider task to populate cache, but leave publication authority entirely to the command-layer query token in Task 5.

**Distinct test coverage:** fresh cache performs zero provider calls; stale cache triggers one refresh; failure uses stale through exact seven-day boundary; expired/missing cache returns unavailable; unsupported code does not use stale; direct/inverse simultaneous callers make one request and receive reciprocal rates; changing only amount reuses rate; leader caller cancellation still settles follower; every failure removes in-flight state; persistence failure still returns current rate; no lock is held across injected blocking operations.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml quick_evaluation::currency::service_tests`

### Task 5: Host State, Async Query Ownership, Currency Result, And Icon

**Files:** Modify `src-tauri/src/lib.rs`, `src-tauri/src/model.rs`, `src-tauri/src/commands.rs`, `src/protocol.ts`, `src/launcher-core.ts`, `src/launcher-view.tsx`, `src/styles.css`, `src/launcher.test.tsx`.

**Dependencies:** Task 4; design sections `Common Query Contract / Classification Result`, `Currency Failure Behavior`, `Launcher Result And Wire Contracts`, `UI States`, `Data Flow`, and `Testing Strategy / Currency Tests` and `Protocol And UI Tests`.

- [ ] Load one `Arc<ExchangeRateService>` from the app data directory during host setup and manage it exactly once. Cache load failure produces an empty service and does not abort startup.
- [ ] In `search_apps`, classify currency before local date/time/unit/arithmetic. Begin the application-domain query token before awaiting the service, then publish only through `publish_if_latest`; `NoMatch` continues existing routing.
- [ ] Add `Currency` to the shared intent/dispatcher as the first recognizer in the same task that adds the async command resolution path and backend icon mapping.
- [ ] Add an async test seam accepting a deterministic exchange-rate resolver so command-level fresh/stale/error/late-owner behavior is testable without a Tauri window or live provider.
- [ ] Add exact Rust/TypeScript `currency` icon kind, sanitizer, and Lucide currency-exchange rendering with the existing 28 px wrapper, semantic green token/soft background, and forced-colors behavior.
- [ ] Publish success as an exclusive executable row with private `CopyBuiltInText`; publish unsupported/unavailable as exclusive informational rows with no action, favorite, refresh metadata, or raw provider text.
- [ ] Keep the existing visible search-pending treatment for a cold request. A cache hit publishes without network/disk blocking; a late response cannot replace newer input, alter selection, execute, or hide.
- [ ] Add the manual smoke-check instructions as test comments or plan completion notes only; do not put a live Frankfurter call in automated tests.

**Distinct test coverage:** service managed exactly once; cache-load failure startup; `100 USD to CNY` end-to-end publication; currency classifier precedence and partial fallthrough; no application snapshot/decorate for recognized currency; fresh/cold/stale/unsupported/unavailable DTOs; query sequence changes while provider is pending and old response is discarded; exact copy-before-hide behavior; informational no-op; currency icon contract/render/accessibility/fallback; no favorite/context menu/refresh metadata.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml commands::tests::currency`; `cargo test --manifest-path src-tauri/Cargo.toml host_manages_exchange_rate_service_once_and_cache_failure_is_nonfatal`; `npm.cmd test -- --run src/launcher.test.tsx -t "currency conversion|currency icon"`

## Final Checklist

- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib`
- [ ] `npm.cmd test -- --run`
- [ ] `npm.cmd run build`
- [ ] `cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --bin uipilot`
- [ ] Review the approved specification's full ten-item `Acceptance Criteria` after all four plans are complete.
- [ ] Before a manual provider smoke check, tell the user it will contact Frankfurter; before visual acceptance, tell the user it requires opening the real launcher. Never synthesize input or control the user's mouse/keyboard.
- [ ] Confirm each task commit contains only planned files and preserves pre-existing user changes.
