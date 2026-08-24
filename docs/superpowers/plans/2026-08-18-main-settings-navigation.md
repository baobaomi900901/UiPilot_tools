# Main And Settings Navigation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add direct, same-window navigation from the launcher query to Settings and back while preserving the query and restoring its results and focus.

**Architecture:** `LauncherCore` remains the single view-state owner. Native `launcher://shown` events and the new UI navigation method share one transition helper so epoch changes, pending-work retirement, settings loading, query refresh, and focus effects cannot diverge.

**Tech Stack:** React `19.2.7`, Ant Design `6.5.1`, Lucide React `^1.21.0`, Vitest `4.1.10`.

**Approved design:** [`docs/superpowers/specs/2026-08-18-main-settings-navigation-design.md`](../specs/2026-08-18-main-settings-navigation-design.md), especially `User Contract`, `Architecture`, `Failure Behavior`, and `Acceptance`.

## Global Constraints

- Keep the existing native window visible; do not invoke a new native show operation.
- Preserve the launcher query across both transitions and refresh non-empty query results on return.
- Keep Settings Close and `Escape` behavior unchanged: both still hide the main window.
- Preserve existing dimensions, drag regions, theme tokens, settings scrolling, and request ownership checks.
- Use Lucide icons through existing Ant Design controls; add no dependency.
- Do not synthesize input or control the user's mouse or keyboard.

## Global Execution Rules

- Follow focused TDD: add failing core and view assertions, confirm the intended failures, implement the minimum contract, and rerun the focused suite.
- Produce one atomic implementation commit only from this task's hunks; do not include pre-existing worktree changes.
- Run `npm.cmd test -- --run src/launcher.test.tsx` for focused verification and the full frontend test/build gate once at completion.
- Dependency order is the single Task 1 followed by final verification.

---

### Task 1: Same-Window Launcher And Settings Navigation

**Files:** `src/launcher-core.ts`, `src/launcher-view.tsx`, `src/launcher.test.tsx`, `src/styles.css`.

**Dependencies:** Approved design sections `User Contract`, `Architecture`, `Failure Behavior`, and `Testing`.

- [ ] Add `LauncherCore.navigate(target: 'launcher' | 'settings'): void` and route it through the same internal transition helper used by validated native shown events.
- [ ] Preserve the active invocation and query for local navigation while retiring stale results and asynchronous UI owners through the existing epoch/token rules.
- [ ] On launcher-to-Settings, queue the existing settings load; on Settings-to-launcher, schedule a fresh search only when the retained query is non-empty.
- [ ] Add a Lucide `Settings` button as the launcher `Input` suffix and connect it to `core.navigate('settings')` without changing native text binding.
- [ ] Add a Lucide `ArrowLeft` button immediately before the Settings title and connect it to `core.navigate('launcher')`; retain the right-side Close button and `Escape` handling.
- [ ] Add only the minimal suffix/title-group styles needed for stable icon-button dimensions and existing drag/no-drag behavior.

**Distinct test coverage:** Launcher with retained query and published results -> Settings retires results and starts settings loading without calling `hideLauncher`; Settings -> launcher retains the query, starts a fresh search, and restores input focus; Settings and Back controls render as Lucide icons with accessible labels in the required positions; Close and `Escape` still call the existing hide path.

**Verify:** `npm.cmd test -- --run src/launcher.test.tsx`

## Final Checklist

- [ ] Focused launcher tests pass.
- [ ] Full frontend tests and production build pass.
- [ ] `git diff --check` passes.
- [ ] User is notified to test the real Tauri window; no automated foreground or input control is used.
