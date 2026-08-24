# Built-in Result Icons Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the generic square for `/find`, calculator, and browser-search results with stable colored vector icons.

**Architecture:** Carry an optional exact `iconKind` through the existing Rust result DTO and frontend snapshot. Render the three host-owned kinds with the already-installed `@ant-design/icons`; keep application PNG icons and the existing fallback unchanged.

**Tech Stack:** Rust, Serde, TypeScript, React, Ant Design Icons 6.3.2, Vitest.

**Approved specification:** [2026-08-17-built-in-result-icons-design.md](../specs/2026-08-17-built-in-result-icons-design.md)

## Global Constraints

- Exact values are `find | calculator | webSearch`; omission or an injected unknown value remains backward-compatible through the existing fallback.
- `iconKind` is presentation-only and cannot affect identity, ordering, authorization, execution, or persistence.
- Result icons remain in the existing 28 by 28 layout box and are decorative to assistive technology.
- Ordinary application PNG icons and load-error fallback remain unchanged.
- Do not start the GUI, synthesize input, or control the user's mouse or keyboard.

## Global Execution Rules

- Follow focused TDD once per task: add the distinct failing tests, confirm the intended failure, implement the minimum contract, and rerun the focused tests.
- Preserve all pre-existing worktree changes. Because the implementation overlaps existing uncommitted files, do not create implementation commits unless the user explicitly requests consolidation.
- Dependency order is `Task 1 -> Task 2`.

---

### Task 1: Semantic Result Icon Contract

**Files:** `src-tauri/src/model.rs`, `src-tauri/src/commands.rs`, other Rust result constructors identified by compilation, `src/protocol.ts`, `src/launcher-core.ts`, `src/launcher.test.tsx`

**Dependencies:** Design sections `Architecture` and `Data And Failure Behavior`.

- [ ] Add the exact optional `ResultIconKind`/`iconKind` wire contract and preserve omission for all ordinary results.
- [ ] Publish `calculator` for built-in calculations and `webSearch` for browser-search results.
- [ ] Assign `find` to the frontend-owned local `/find` result and carry all three values into `LauncherSnapshot`.
- [ ] Keep plugin and application producers on `None`; do not infer kinds from titles.

**Distinct test coverage:** the typed contract carries the three exact values; omission and an injected unknown value fall back; calculator and browser-search DTOs expose the right kind; local `/find` snapshots as `find`; application results remain PNG-backed with no semantic kind.

**Verify:** `cargo test commands::tests::browser_search_result_snapshots_selected_engine` and `npm.cmd test -- --run src/launcher.test.tsx -t "result icon kind"`

### Task 2: Colored Vector Rendering

**Files:** `src/launcher-view.tsx`, `src/styles.css`, `src/launcher.test.tsx`

**Dependencies:** Task 1; design sections `Visual Design` and `Acceptance`.

- [ ] Add a small built-in result icon renderer using `FolderOpenTwoTone`, `CalculatorTwoTone`, `ChromeOutlined`, and overlaid `SearchOutlined` badges.
- [ ] Use stable CSS dimensions and distinct fixed colors while preserving selected-row contrast and forced-colors usability.
- [ ] Render semantic icons before PNG/fallback handling; leave ordinary application image error recovery intact.
- [ ] Update the existing React source-boundary assertion to permit the approved icon dependency without permitting Tauri APIs or unrelated component families.

**Distinct test coverage:** each semantic kind renders its expected Ant icon classes; composite badges remain inside one 28 by 28 wrapper; PNG and square fallback paths remain available; decorative icons are hidden from assistive technology.

**Verify:** `npm.cmd test -- --run src/launcher.test.tsx -t "built-in result icons"`

## Final Verification

- [ ] `cargo fmt -- --check`
- [ ] Focused Rust tests for result publication pass.
- [ ] `npm.cmd test -- --run`
- [ ] `npm.cmd run build`
- [ ] `cargo build --no-default-features --bin uipilot`
- [ ] Ask the user to visually confirm the three colored icons; do not launch or operate the GUI automatically.
