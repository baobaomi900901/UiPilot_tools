# Launcher City Time Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve approved English city aliases and canonical IANA names to current `HH:mm` results that refresh safely at each minute boundary.

**Architecture:** Extend the shared quick-evaluation module from [Launcher Date Quick Evaluation And Shared Foundation](2026-09-03-launcher-date-quick-evaluation.md) with a bundled IANA-backed local evaluator. Add explicit response-level refresh metadata so launcher-core schedules a guarded quiet re-query without deriving behavior from presentation icons. The approved [Launcher Quick Evaluation Design](../specs/2026-09-03-launcher-quick-evaluation-design.md) defines aliases, output, ownership, timer cancellation, and failure behavior.

**Tech Stack:** Rust 1.96/Tauri 2, `chrono` 0.4.45, `chrono-tz` 0.10.4 with `case-insensitive`, TypeScript 7, React 19, Lucide React, Vitest fake timers, Rust unit tests.

## Global Constraints

- The date/shared-foundation plan must be complete first. Unit conversion may be present but is not a semantic dependency.
- Follow design sections `City And Time-Zone Time`, `Launcher Result And Wire Contracts / Refresh Metadata`, `Data Flow`, and `Failure And Safety Rules` exactly.
- Do not call a time/geocoding API or read public-plugin network state. Time-zone rules come only from the bundled IANA database.
- Aliases ignore ASCII case, spaces, hyphens, and underscores; full IANA names resolve case-insensitively to canonical spelling.
- Reject ambiguous `CST/EST/IST`; accept explicit UTC and GMT.
- Display fixed 24-hour `HH:mm` without seconds and refresh once at the next minute boundary only while the same launcher owner remains current.
- Refresh behavior comes from validated `refreshAtUtc`, never from `iconKind`, result text, or a guessed interval.
- Preserve informational/no-action errors, private copy authorization, no favorites, UI geometry, and all existing query-sequence limits.

## Global Execution Rules

- Dependency order is `Task 1 -> Task 2 -> Task 3`.
- Every task follows focused TDD and produces one atomic task-only commit; review fixes use separate commits.
- Each task receives specification-compliance and code-quality review before its dependent task begins when available.
- Use the focused commands listed below, then run the final checklist once.
- Preserve pre-existing worktree changes. Do not launch or control the real GUI.

---

### Task 1: IANA Catalog, Alias Parser, And Time Evaluation

**Files:** Modify `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/src/quick_evaluation/mod.rs`; create `src-tauri/src/quick_evaluation/time_zone/mod.rs`, `src-tauri/src/quick_evaluation/time_zone/aliases.rs`.

**Dependencies:** Shared foundation; design sections `City And Time-Zone Time / User Grammar`, `Initial Alias Catalog`, and `Clock And Time-Zone Data`.

- [ ] Add exact compatible dependency entries for `chrono = "0.4.45"` and `chrono-tz = { version = "0.10.4", features = ["case-insensitive"] }`; commit the resolved lockfile with this task.
- [ ] Encode every approved alias as one normalized key to one canonical `chrono_tz::Tz`; register both Sao Paulo spellings explicitly and reject duplicate normalized keys during tests.
- [ ] Parse the terminal English `time` keyword, normalized aliases, and full IANA names. Unknown/ambiguous complete time expressions return typed failures rather than `NoMatch` or guesses.
- [ ] Evaluate from an injected UTC instant and format `HH:mm`, ISO date, approved English display name or canonical IANA name, numeric `UTC±HH:MM`, and active abbreviation.
- [ ] Compute the next UTC minute boundary with checked arithmetic and emit it in strict `...Z` RFC 3339 form as the internal result refresh timestamp.
- [ ] Produce the shared successful/invalid result structures and local-clock failure, but leave global dispatcher registration to Task 2 so the backend intent and icon mapping land in one compiling task.

**Distinct test coverage:** every alias and separator/case variant; duplicate-normalization guard; canonical IANA case-insensitive lookup; UTC/GMT; rejection of CST/EST/IST and unknown zones; New York winter/summer offsets and both DST transition boundaries; direct-IANA display name; exact `HH:mm`, date, abbreviation, copy text, and next-minute UTC timestamp; minute rollover across date/year.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml quick_evaluation::time_zone`

### Task 2: Backend Time Result And Refresh Wire Contract

**Files:** Modify `src-tauri/src/model.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/plugins.rs`, `src-tauri/src/result_registry.rs`.

**Dependencies:** Task 1; design sections `Minute Refresh`, `Launcher Result And Wire Contracts / Semantic Icons`, `Refresh Metadata`, and `Data Flow`.

- [ ] Add optional `refresh_at_utc`/`refreshAtUtc` to the Rust `SearchResponse` wire model and set `None` explicitly in every non-time response constructor in commands, plugin publication, and registry tests.
- [ ] Add exact Rust `TimeZone` result icon kind and map the internal kind without exposing the IANA zone as an executable private action.
- [ ] Register `TimeZone` in the shared intent/dispatcher after date and before unit/arithmetic in the same task that adds backend result publication.
- [ ] Have the common quick publisher copy a valid internal refresh timestamp only for a successful time result. Invalid time rows and every other result kind omit it.
- [ ] Capture one UTC instant per evaluation, begin one current query owner, and publish title/copy action/refresh metadata from that same instant so displayed and copied minutes cannot disagree.
- [ ] Preserve stale-query behavior: a response that loses registry ownership returns no current publication and cannot schedule refresh metadata for the new query.

**Distinct test coverage:** serialization omission/presence for all result kinds; all existing SearchResponse constructors remain wire-compatible; time title, copy action, subtitle, and refresh timestamp derive from one injected instant; invalid zones omit action/refresh; stale time publication returns no response; non-time quick and plugin/application responses never acquire refresh metadata.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml commands::tests::time`; `cargo test --manifest-path src-tauri/Cargo.toml result_registry::tests`; `cargo test --manifest-path src-tauri/Cargo.toml plugins::tests`

### Task 3: Frontend Minute Scheduler And Time Icon

**Files:** Modify `src/protocol.ts`, `src/launcher-core.ts`, `src/launcher-view.tsx`, `src/styles.css`, `src/launcher.test.tsx`.

**Dependencies:** Task 2; design sections `Minute Refresh`, `Launcher Result And Wire Contracts / Refresh Metadata`, `UI States`, and `Testing Strategy / Time-Zone Tests` and `Protocol And UI Tests`.

- [ ] Add optional `refreshAtUtc` and exact `timeZone` icon kind to TypeScript contracts and field sanitizers. Validate a real strict UTC RFC 3339 timestamp; reject only the bad field and retain otherwise valid rows.
- [ ] Track one refresh timeout and its exact invocation ID, view epoch, control key/value, query sequence, and accepted response owner. Cancel it before ordinary queries and on edit, navigation, hide, sequence exhaustion, or destroy.
- [ ] At the deadline, recheck every owner, increment the sequence once, visually retain the prior row but temporarily disable its default action, and start one search with `showPending = false`.
- [ ] Accept only the matching quiet response, replace the row, restore the current copy action, and schedule its new boundary. A late wake sends one query and never loops through missed minutes.
- [ ] Ensure rejection/throw of a quiet refresh clears temporary pending/disabled state without hiding the launcher; it may leave the last visible minute but must not leave an executable retired action.
- [ ] Render `timeZone` using a Lucide clock icon in the stable wrapper with an existing semantic token/soft background and forced-colors behavior.

**Distinct test coverage:** strict valid/invalid/stale refresh timestamps; fake-timer fire exactly at the minute; owner changes for raw input, invocation, view, and sequence each suppress the request; hide/destroy cancellation; late wake performs once; quiet refresh does not set visible spinner; retained row is non-executable until replacement; old response cannot replace newer text; exact clock icon and accessibility treatment.

**Verify:** `npm.cmd test -- --run src/launcher.test.tsx -t "time zone|minute refresh|time icon"`

## Final Checklist

- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib`
- [ ] `npm.cmd test -- --run src/launcher.test.tsx`
- [ ] `npm.cmd run build`
- [ ] Confirm design acceptance items 5, 8, 9, and 10 with injected clocks; notify the user before requesting any real visual acceptance.
- [ ] Confirm each task commit contains only planned files and preserves pre-existing user changes.
