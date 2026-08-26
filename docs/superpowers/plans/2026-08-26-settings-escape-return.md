# Settings Escape Return Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Escape in the Settings view return to the launcher while preserving launcher and Panel Escape behavior.

**Architecture:** Keep React's existing Settings key event forwarding unchanged. Add the view distinction to the shared launcher-core Escape route so keyboard return and the existing back button converge on the same local navigation transition.

**Tech Stack:** TypeScript, React 19, Vitest, jsdom.

**Approved design:** [`docs/superpowers/specs/2026-08-26-settings-escape-return-design.md`](../specs/2026-08-26-settings-escape-return-design.md), especially `Contract`, `Implementation Boundary`, and `Verification`.

## Global Constraints

- Settings Escape must reuse local `navigate('launcher')` semantics.
- IME composition remains inert; launcher Escape continues hiding the window.
- Panel-content Escape and native window lifecycle are unchanged.
- Automated tests must not synthesize real OS mouse or keyboard input.

## Global Execution Rules

- Follow TDD: update focused failing assertions, confirm the intended failure, implement the minimum branch, rerun focused tests, and commit.
- Produce one atomic implementation commit containing only the files named below.
- Dependency order: Task 1 only.

---

### Task 1: Route Settings Escape Through Local Navigation

**Files:**
- Modify: `src/launcher-core.ts`
- Test: `src/launcher.test.tsx`

**Dependencies:** Approved design sections `Contract`, `Implementation Boundary`, and `Verification`.

- [ ] Change the existing core and React Settings Escape expectations from `hideLauncher` to `view: 'launcher'`, no hide call, cleared query, and restored combobox focus.
- [ ] Preserve explicit coverage that IME-composing Escape does nothing and launcher-view Escape still calls `hideLauncher`.
- [ ] In `keyDown`, route non-composing Settings Escape through `navigate('launcher')`; retain `requestHide()` for every other main-window view.

**Distinct test coverage:** Starting from a live Settings invocation, Escape transitions locally to a clean launcher view and does not call `hideLauncher`; dispatching Escape from a focused Settings tab restores focus to the launcher combobox; composition and launcher-view outcomes remain unchanged.

**Verify:** `npm test -- src/launcher.test.tsx`

## Final Checklist

- [ ] Settings Escape matches the existing back button on all Settings tabs.
- [ ] Launcher and Panel Escape contracts are unchanged.
- [ ] `npm run build` passes.
- [ ] User performs real-window focus acceptance; automation does not control input.
