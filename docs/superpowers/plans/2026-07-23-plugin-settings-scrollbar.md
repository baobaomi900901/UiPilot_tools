# Plugin Settings Scrollbar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the plugin settings page scrollbar match the main result list scrollbar exactly.

**Architecture:** Keep `.settings-form` as the existing scroll owner and group it with `.result-list` in the existing CSS scrollbar rules. Add no component, JavaScript, nested scroll area, or dependency.

**Tech Stack:** CSS, Vitest source assertions.

## Global Constraints

- Work only in `D:\code\UiPilot_tools\.worktrees\plugin-management-settings`.
- Reuse the existing 6px scrollbar, transparent track, thumb colors, radius, dark mode, and forced-colors rules.
- Do not start the GUI or merge to `main`.

---

### Task 1: Share The Main Scrollbar Style

**Files:**
- Modify: `src/styles.css`
- Test: `src/launcher.test.tsx`

**Interfaces:**
- Consumes: `.result-list` and `.settings-form` CSS selectors.
- Produces: identical native WebKit scrollbar styling for both scroll containers.

- [x] **Step 1: Extend the existing scrollbar regression assertion**

Add assertions requiring `.settings-form` beside `.result-list` in the base, dark-mode, and forced-colors scrollbar selectors.

- [x] **Step 2: Run the focused test and verify failure**

Run: `npm test -- --run src/launcher.test.tsx -t "keeps the slim result scrollbar visible without hover"`

Expected: FAIL because `.settings-form` is not yet included in the scrollbar selectors.

- [x] **Step 3: Share the existing CSS rules**

Group `.settings-form` with `.result-list` for the scrollbar custom property, `::-webkit-scrollbar`, track, thumb, dark-mode thumb variable, and forced-colors thumb rule. Keep the existing values unchanged.

- [x] **Step 4: Verify**

Run:

```powershell
npm test -- --run src/launcher.test.tsx
npm run build
```

Expected: all tests and production build pass.

- [x] **Step 5: Commit**

```powershell
git add src/styles.css src/launcher.test.tsx docs/superpowers/plans/2026-07-23-plugin-settings-scrollbar.md
git commit -m "style: match plugin settings scrollbar"
```
