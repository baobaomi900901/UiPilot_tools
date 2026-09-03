# Launcher Date Quick Evaluation And Shared Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Introduce the shared quick-evaluation router and add strict `today`/ISO calendar arithmetic while preserving existing arithmetic and launcher routing.

**Architecture:** The approved [Launcher Quick Evaluation Design](../specs/2026-09-03-launcher-quick-evaluation-design.md) is the source of truth. A focused Rust `quick_evaluation` module owns classification and local evaluation; the existing launcher search command remains the sole publisher of result DTOs and private copy actions. This first vertical slice establishes the shared three-state contract, informational rows, and semantic-icon path used by the later unit, time-zone, and currency plans.

**Tech Stack:** Rust 1.96/Tauri 2, existing `time` 0.3, TypeScript 7, React 19, Lucide React, Vitest, Rust unit tests.

## Global Constraints

- Follow design sections `Common Query Contract`, `Date Arithmetic`, `Launcher Result And Wire Contracts`, `UI States`, and `Failure And Safety Rules` exactly.
- English syntax only; fixed launcher/error text is Chinese and weekdays are English full names.
- Preserve the existing four-function grammar, result text, copy action, 400-byte ceiling, and compatibility case `2026-08-16 -> 2002`.
- Quick-evaluation ownership is `currency -> date -> time zone -> unit -> arithmetic`; this slice implements date and arithmetic while leaving explicit extension points for the other kinds.
- `NoMatch` continues ordinary launcher search; `Invalid` and `Ready` each publish one exclusive row.
- Successful rows use registry-authorized `CopyBuiltInText`; informational rows have no `ResultAction` and never call `execute_result`.
- Do not change public-plugin API/SDK contracts, favorites, app identifiers, or unrelated launcher ordering.
- Use existing theme tokens and 28 px built-in icon geometry; do not add hard-coded theme colors or run real-window automation.

## Shared Contract Overview

Define the common types once in `quick_evaluation/mod.rs`: a semantic `QuickKind`, `QuickClassification::{NoMatch, Invalid, Ready}`, typed `QuickIntent`, `QuickFailure`, and successful `QuickOutput` containing title, subtitle, exact copy text, and optional refresh timestamp. The date and arithmetic handlers return intents/results through these types; later plans extend the closed intent and kind enums instead of bypassing the dispatcher.

Expose one lexical helper in `number.rs` that validates the approved English numeric grammar and returns a canonical comma-free string. Date quantities consume its non-negative integer subset. Unit and currency plans reuse the same lexical boundary without changing accepted syntax.

## Global Execution Rules

- Dependency order is `Task 1 -> Task 2 -> Task 3`.
- Every task follows focused TDD: add its distinct failing tests, confirm the intended failure, implement the minimum contract, rerun focused tests, and make one atomic commit containing only that task.
- Each task receives specification-compliance and code-quality review before its dependent task begins when the active workflow provides those gates.
- Use `cargo test --manifest-path src-tauri/Cargo.toml <filter>` for Rust and `npm.cmd test -- --run <file> -t <filter>` for frontend focus.
- Do not absorb pre-existing worktree changes. Review fixes use separate commits.

---

### Task 1: Pure Quick-Evaluation, Number, And Date Modules

**Files:** Create `src-tauri/src/quick_evaluation/mod.rs`, `src-tauri/src/quick_evaluation/number.rs`, `src-tauri/src/quick_evaluation/date.rs`; modify `src-tauri/src/lib.rs`.

**Dependencies:** Design sections `Common Query Contract / Normalization`, `Number Grammar`, `Classification Result`, and `Date Arithmetic / User Grammar` and `Calendar Semantics`.

- [ ] Add the shared classification, intent, failure, output, and semantic-kind types described in `Shared Contract Overview`; keep UI DTO and result-registry types outside this module.
- [ ] Implement the canonical numeric lexer with strict thousands grouping, dot decimals, optional leading sign, finite lexical bounds, and integer-subset validation; reject exponent, comma-decimal, Unicode digit, NaN, and infinity forms.
- [ ] Implement strict `today | YYYY-MM-DD`, one operator, integer quantity, and singular/plural day/week/month/year parsing. Return `NoMatch` for incomplete shapes and a typed `Invalid` failure only after the date signature is complete.
- [ ] Evaluate against an injected `time::Date`; use checked day/week arithmetic, checked quantity multiplication, year range `0001..=9999`, and approved month/year end-of-month clamping.
- [ ] Format the result as ISO title, English weekday plus normalized expression subtitle, and identical ISO copy text. Production local-date acquisition remains outside the pure evaluator for Task 2.
- [ ] Register `mod quick_evaluation` beside the still-existing `mod calculator`; Task 2 performs the arithmetic move and removes the legacy source module only when all call sites change in the same compiling task.

**Distinct test coverage:** every approved numeric form and rejection; date `NoMatch/Invalid/Ready` boundaries; day/week cross-month and cross-year; January 31 month clamping; leap-day year clamping; invalid ISO dates; fractional/negative quantities; checked overflow; exact title, weekday, subtitle, and copy text.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml quick_evaluation::`

### Task 2: Backend Classification, Informational Activation, And Publication

**Files:** Create `src-tauri/src/quick_evaluation/arithmetic.rs`; delete `src-tauri/src/calculator.rs`; modify `src-tauri/src/model.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`.

**Dependencies:** Task 1; design sections `Relationship To Existing Launcher Contracts`, `Common Query Contract / Classification Result`, `Launcher Result And Wire Contracts / Activation And Copy`, `Data Flow`, and `Failure And Safety Rules`.

- [ ] Move the current arithmetic parser and its characterization tests into `quick_evaluation/arithmetic.rs` without changing accepted expressions or output formatting; replace `mod calculator` and its structural assertion only in this same integration task.
- [ ] Extend the host activation enum with exact unit variant `Informational`; serialize it as `{ "kind": "informational" }` and keep it separate from registry-authorized `ExecuteResult`.
- [ ] Add one `publish_quick_evaluation` adapter that maps `Ready` to an `ExecuteResult` row plus private `CopyBuiltInText`, maps `Invalid` to an `Informational` row with no action and `has_default_action = false`, and never exposes private copy text outside the title/copy result contract.
- [ ] Move quick classification ahead of empty/slash/plain handling for non-slash input, while preserving the approved outer classification order. Remove the old calculator-only branch from `search_apps_with_catalog`.
- [ ] Obtain `today` from Windows local time once per date evaluation. A local-time failure publishes the approved explicit informational error and never substitutes UTC.
- [ ] Rework the test-only search helper so date/arithmetic exclusivity and ordinary fallthrough can be tested without a Tauri window or application cache.
- [ ] Preserve query-token ownership: recognized local evaluation begins/publishes one current application-domain query, stale publication returns no response, and `NoMatch` lets the existing catalog start its normal owner.
- [ ] Keep successful clipboard ordering unchanged: resolve current action, write text, then clear-and-hide; informational activation must be rejected before `execute_result` is called.

**Distinct test coverage:** arithmetic regression table including `.5*8`, incomplete/divide-by-zero/unsupported operators, and `2026-08-16`; Rust serialization of `Informational`; successful and invalid date rows; no action registered for informational rows; Enter-copy text ownership for date/arithmetic; valid quick queries do not snapshot/decorate applications; incomplete date expressions retain Find/web/plugin/application behavior; empty and every slash-command path remain unchanged; stale local publication cannot replace a newer owner; local-time acquisition failure does not use UTC.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml commands::tests`; `cargo test --manifest-path src-tauri/Cargo.toml model::tests`; `cargo test --manifest-path src-tauri/Cargo.toml host_uses_builtin_calculator_without_legacy_math_command`

### Task 3: Frontend Informational Rows And Date Icon

**Files:** Modify `src/protocol.ts`, `src/launcher-core.ts`, `src/launcher-view.tsx`, `src/styles.css`, `src/launcher.test.tsx`.

**Dependencies:** Task 2; design sections `Launcher Result And Wire Contracts / Semantic Icons`, `Activation And Copy`, `UI States`, and `Testing Strategy / Protocol And UI Tests`.

- [ ] Add `informational` to the TypeScript activation union and strict `safeLauncherActivation` parser. Retain such rows only when `hasDefaultAction` is exactly false; never route them to completion, Find, panels, plugins, or `executeResult`.
- [ ] Add `date` to the host-only `ResultIconKind` union and field sanitizer. Unknown kinds continue to drop only the field and use the existing icon fallback.
- [ ] Render `date` with a Lucide calendar icon inside the existing 28 px wrapper, semantic blue token/soft background, decorative accessibility treatment, and forced-colors behavior.
- [ ] Keep the one informational row selectable for announcement and inspection; Enter and click may update selection but perform no side effect or hide.
- [ ] Extend the existing launcher response, icon, activation, keyboard, mouse, status-announcement, and malformed-item tests without creating a second result list or frontend-synthesized row.

**Distinct test coverage:** valid and malformed informational wire values; execute rows still require non-empty IDs; informational Enter/click performs no client call; date icon renders the expected Lucide class at stable dimensions and is hidden from assistive technology; unknown icon falls back; success copies exact ISO text and failure keeps the launcher visible.

**Verify:** `npm.cmd test -- --run src/launcher.test.tsx -t "quick evaluation|informational|date icon"`

## Final Checklist

- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib`
- [ ] `npm.cmd test -- --run src/launcher.test.tsx`
- [ ] `npm.cmd run build`
- [ ] Confirm design acceptance items 4, 8, 9, and 10 for this slice without controlling the user's GUI.
- [ ] Confirm the three task commits contain only planned files and preserve all pre-existing user changes.
