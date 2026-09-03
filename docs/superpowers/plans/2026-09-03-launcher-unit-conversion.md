# Launcher Unit Conversion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add strict local conversion across the approved length, mass, area, volume, and temperature units, including US defaults and explicit Imperial volume prefixes.

**Architecture:** Build on the shared classifier/result boundary introduced by [Launcher Date Quick Evaluation And Shared Foundation](2026-09-03-launcher-date-quick-evaluation.md). A data-focused unit catalog resolves aliases to typed dimensions and definitions; a separate evaluator converts through one base unit per dimension and formats one host-owned quick result. The approved [Launcher Quick Evaluation Design](../specs/2026-09-03-launcher-quick-evaluation-design.md) remains authoritative.

**Tech Stack:** Rust 1.96/Tauri 2, TypeScript 7, React 19, Lucide React, Vitest, Rust unit tests; no new runtime dependency.

## Global Constraints

- The date/shared-foundation plan must be complete first.
- Follow design sections `Common Query Contract`, `Unit Conversion`, `Launcher Result And Wire Contracts`, `UI States`, and `Failure And Safety Rules` exactly.
- Support only the approved five dimensions and listed unit catalog; no user-defined or guessed units.
- Bare `gal/qt/pt/fl oz` and `ton` are US; `cup` is US customary `236.5882365 mL`; `tonne/metric ton` is metric.
- `imp/uk/imperial` applies only to Imperial `fl oz/pt/qt/gal`; no general Imperial length/mass expansion.
- Unit output uses at most six significant digits, the approved scientific thresholds, canonical symbols, trailing-zero removal, and negative-zero normalization.
- Unit conversion is local and synchronous; it must not touch disk, network, settings, plugin services, or application discovery.
- Preserve the shared `NoMatch/Invalid/Ready`, informational activation, private copy action, no-favorite, and query-owner rules.

## Global Execution Rules

- Dependency order is `Task 1 -> Task 2 -> Task 3`.
- Every task follows focused TDD and produces one atomic task-only commit. Review fixes use separate commits.
- Each task receives specification-compliance and code-quality review before its dependent task begins when available.
- Rust focused commands use `cargo test --manifest-path src-tauri/Cargo.toml <filter>`; frontend focus uses `npm.cmd test -- --run src/launcher.test.tsx -t <filter>`.
- Preserve all pre-existing worktree changes and do not run real-window automation.

---

### Task 1: Typed Unit Catalog And Alias Resolution

**Files:** Create `src-tauri/src/quick_evaluation/units/mod.rs`, `src-tauri/src/quick_evaluation/units/catalog.rs`; modify `src-tauri/src/quick_evaluation/mod.rs`.

**Dependencies:** Shared foundation; design sections `Unit Conversion / User Grammar`, `Supported Units`, and `US Defaults And Imperial Prefixes`.

- [ ] Define closed `Dimension`, `Unit`, linear factor, and temperature-scale types; aliases resolve to typed units before any arithmetic occurs.
- [ ] Encode every canonical metric and US unit plus the four explicitly prefixed Imperial volume families from the approved table. Keep constants and canonical output symbols in one catalog.
- [ ] Normalize conventional abbreviations, English singular/plural names, meter/metre and liter/litre spellings, optional temperature degree sign, area `²/2/^2/sq/square`, volume `³/3/^3/cubic`, and `fl oz/floz/fluid ounce` forms.
- [ ] Parse optional attached number/unit spacing and a case-insensitive ASCII `to` separator without removing meaningful spaces inside compound unit names.
- [ ] Enforce ambiguity rules before conversion: `oz` is mass, `fl oz` is volume, bare `ton` is short ton, `cup` has no regional override, and undefined prefix/unit pairs are `Invalid` rather than guessed.
- [ ] Return `NoMatch` unless the `to` form is complete and at least one side resolves to a known unit; return a typed unknown-unit or dimension failure once the signature is owned.

**Distinct test coverage:** every canonical abbreviation, full name, plural, spelling, exponent, square/cubic, and fluid-ounce alias; attached/spaced number forms; default and explicit-US volume; each `imp/uk/imperial` variant; rejection of metric/Japanese/legal cup names, `imp cup`, bare ambiguous text, unknown units, and malformed exponent/prefix combinations.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml quick_evaluation::units::catalog`

### Task 2: Conversion Arithmetic And Result Formatting

**Files:** Create `src-tauri/src/quick_evaluation/units/format.rs`; modify `src-tauri/src/quick_evaluation/units/mod.rs`, `src-tauri/src/quick_evaluation/mod.rs`.

**Dependencies:** Task 1; design sections `Unit Conversion / Conversion Definitions` and `Unit Display`.

- [ ] Implement one base-unit conversion path for length, mass, area, and volume using the exact anchor definitions from the design. Derive foot/yard/mile, ounce/short-ton, acre, US sub-gallon, Imperial sub-gallon, and cubic factors rather than entering inconsistent rounded duplicates.
- [ ] Implement Celsius/Fahrenheit/Kelvin as affine conversions and reject source values below the exact absolute-zero boundaries before formatting.
- [ ] Reject cross-dimension conversion explicitly; allow same-unit and same-system conversion without special cases that change precision.
- [ ] Format finite results to at most six significant digits; use fixed decimal in the inclusive/exclusive approved normal range, normalized scientific notation outside it, trim insignificant zeroes, and canonicalize negative zero.
- [ ] Produce the shared successful/invalid result structures with title, Chinese dimension subtitle, canonical copy text, and no refresh timestamp; leave global dispatcher registration to Task 3 so every intermediate commit compiles against the then-current icon contract.

**Distinct test coverage:** every exact anchor and representative forward/reverse pair; metric-to-metric and US-to-US; `1000 mm to in -> 39.3701 in`; US and Imperial gallon difference; US customary cup; short ton versus tonne; mass ounce versus fluid ounce; all six temperature directions and absolute-zero edges; cross-dimension failure; fixed/scientific thresholds, six-significant-digit ties, trailing zeroes, and negative zero.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml quick_evaluation::units`

### Task 3: Unit Result Publication And Semantic Icon

**Files:** Modify `src-tauri/src/model.rs`, `src-tauri/src/commands.rs`, `src/protocol.ts`, `src/launcher-core.ts`, `src/launcher-view.tsx`, `src/styles.css`, `src/launcher.test.tsx`.

**Dependencies:** Task 2; design sections `Launcher Result And Wire Contracts / Semantic Icons`, `Activation And Copy`, `UI States`, and `Testing Strategy / Protocol And UI Tests`.

- [ ] Map unit success/failure through the shared backend publisher; success registers exact canonical copy text, while unknown/mismatched/below-zero failures publish informational rows without actions.
- [ ] Register `Unit` in the shared intent/dispatcher immediately before arithmetic in the same task that adds the backend icon mapping.
- [ ] Add exact host-only `unit` icon kind to Rust, TypeScript, the field sanitizer, and the Lucide renderer. Use a ruler icon, the existing 28 px wrapper, a semantic token/soft background, and forced-colors support.
- [ ] Keep unit rows exclusive, non-favoritable, and selected by default; do not infer execution or kind from title/unit text.
- [ ] Add backend routing tests proving unit recognition precedes arithmetic and recognized rows do not read/decorate applications; partial/unowned text must retain ordinary results.
- [ ] Add frontend wire, icon, Enter/click copy, informational no-op, long-subtitle, and accessibility assertions alongside existing built-in-result tests.

**Distinct test coverage:** exact `unit` serialization/sanitization; successful private copy action; mismatched-dimension row has no action; query and result ownership under rapid edits; semantic ruler rendering in Light/Dark/forced colors; malformed icon field retains the row with fallback; no favorite/context menu.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml commands::tests::unit`; `npm.cmd test -- --run src/launcher.test.tsx -t "unit conversion|unit icon"`

## Final Checklist

- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib`
- [ ] `npm.cmd test -- --run src/launcher.test.tsx`
- [ ] `npm.cmd run build`
- [ ] Confirm design acceptance items 6 through 10 for this slice without controlling the user's GUI.
- [ ] Confirm each task commit contains only planned files and preserves pre-existing user changes.
