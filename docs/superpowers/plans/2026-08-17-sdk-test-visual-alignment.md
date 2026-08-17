# sdk-test Visual Alignment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Align UiPilot's launcher, Settings, `/find`, and public-plugin host container with sdk-test's visual language while preserving Ant Design and all existing behavior.

**Architecture:** Introduce one UiPilot-owned semantic token layer and one shared Ant Design theme projection. Migrate the three host React views and public-plugin settings controls to that foundation, replace host action icons with Lucide, then remove the superseded icon dependency and generic per-view palettes.

**Tech Stack:** React `19.2.7`, Ant Design `6.5.1`, Vite `8.1.5`, OverlayScrollbars `2.16.x`, `lucide-react ^1.21.0`.

**Approved design:** [`docs/superpowers/specs/2026-08-17-sdk-test-visual-alignment-design.md`](../specs/2026-08-17-sdk-test-visual-alignment-design.md), especially `Theme Architecture`, `Surface Treatment`, `Icon Contract`, `Failure And Compatibility Behavior`, and `Acceptance Criteria`.

## Global Constraints

- Keep Ant Design; do not add Tailwind, Radix, shadcn/ui, or a cross-repository package.
- Preserve window dimensions, layout tracks, focus transfer, pin/close semantics, auto-hide, keyboard behavior, scrolling, result authorization, and plugin isolation.
- Restyle only host-owned plugin chrome; do not modify plugin-provided HTML or CSS.
- Preserve `system | dark | light` persistence and current final-scheme ownership.
- Preserve existing plugin-facing `--uipilot-color-*` variable names.
- Host action icons use Lucide; built-in launcher result icons retain their accepted semantic colors.
- Do not synthesize input or control the user's mouse or keyboard. Real-window validation waits for explicit user testing.

## Global Execution Rules

- Every task follows focused TDD: add a failing contract test, confirm the intended failure, implement the minimum change, and rerun the focused suite.
- Every task produces one atomic commit containing only that task and no unrelated pre-existing changes.
- Dependency order is `Task 1 -> Task 2 -> Task 3 -> Task 4 -> Task 5`.
- Use `npm.cmd test -- --run <files>` for focused frontend tests and `npm.cmd run build` only at the final integration gate unless a task exposes a type error.
- Treat the approved design as authoritative for visual scope and compatibility; this plan does not redefine runtime behavior.

---

### Task 1: Shared Theme Foundation

**Files:** Create `src/ui-theme.ts`, `src/ui-theme.test.ts`, `src/ui-theme.css`; modify `src/main.ts`, `package.json`, `package-lock.json`.

**Dependencies:** Approved design sections `Theme Architecture`, `Typography And Geometry`, and `Failure And Compatibility Behavior`.

- [ ] Add `lucide-react ^1.21.0` without changing React, Ant Design, OverlayScrollbars, TypeScript, or Vite versions.
- [ ] Define `UiColorScheme`, `resolveUiColorScheme(preference, systemDark)`, the complete light/dark semantic token records, and `uiThemeConfig(scheme)` as pure exports.
- [ ] Project sdk-test's current semantic palette, radii, compact control sizing, typography, motion-off setting, and component states into Ant Design `ThemeConfig`.
- [ ] Define the matching `--uipilot-ui-*` CSS properties for light and dark roots and map the existing plugin-facing `--uipilot-color-*` variables to them.
- [ ] Import the theme stylesheet once before the existing application stylesheet.

**Distinct test coverage:** Exact final-scheme resolution for all theme preferences; complete and non-divergent light/dark semantic token keys; deterministic Ant token projection; sdk-test font/radius contract; package manifests contain Lucide and contain no Tailwind/Radix packages.

**Verify:** `npm.cmd test -- --run src/ui-theme.test.ts src/dev-config.test.ts`

### Task 2: Public-Plugin Host Container

**Files:** Modify `src/plugin-window-view.tsx`, `src/plugin-window-view.test.tsx`, `src/styles.css`.

**Dependencies:** Task 1 exports; approved design sections `Public-Plugin Window Container` and `Icon Contract`.

- [ ] Apply the shared host surface, title-bar, border, text, hover, selected, destructive, and focus-ring tokens to `.plugin-window-shell` without changing its dimensions or drag regions.
- [ ] Replace Ant pin/close glyphs with Lucide `Pin` and `X`; retain tooltips, accessible labels, pending disablement, click handlers, and visually hidden errors.
- [ ] Expose pin state through `aria-pressed` while preserving the existing selected class and core snapshot contract.
- [ ] Confirm the shell continues to publish the existing plugin-facing theme variables and does not style plugin-owned content.

**Distinct test coverage:** Unpinned and pinned Lucide icon states; `aria-pressed` tracks the core snapshot; close and pin still invoke exactly their existing core methods; pending state disables both actions; shell markup keeps drag regions and compatibility variables.

**Verify:** `npm.cmd test -- --run src/plugin-window-view.test.tsx src/plugin-window-core.test.ts`

### Task 3: `/find` Surface

**Files:** Modify `src/find-view.tsx`, `src/find-view.test.tsx`, `src/styles.css`.

**Dependencies:** Tasks 1-2; approved design sections `/find`, `Icon Contract`, and `Failure And Compatibility Behavior`.

- [ ] Replace local theme resolution and inline Ant theme construction with Task 1's shared exports while keeping the existing media listener and snapshot ownership.
- [ ] Replace pin and close glyphs with the same Lucide treatment used by the plugin shell.
- [ ] Apply shared tokens to the find surface, query field, category states, result states, preview, footer, status, scrollbar, and focus ring without changing grid tracks or responsive behavior.
- [ ] Retain all input focus, arrow navigation, category cycling, result execution, preview, pin, close, drag, and auto-hide handlers verbatim in behavior.

**Distinct test coverage:** Shared theme projection in light/dark/system modes; Lucide pin/close semantics; existing focus and keyboard sequences remain unchanged; selected categories/results and forced-colors rules remain present; no geometry contract changes.

**Verify:** `npm.cmd test -- --run src/find-view.test.tsx src/find-core.test.ts`

### Task 4: Launcher And Settings Surfaces

**Files:** Modify `src/launcher-view.tsx`, `src/public-plugin-panel.tsx`, `src/launcher.test.tsx`, `src/styles.css`.

**Dependencies:** Tasks 1-3; approved design sections `Main Launcher`, `Settings`, and `Icon Contract`.

- [ ] Replace local theme resolution and inline Ant theme construction with Task 1's shared exports while preserving the existing media listener and root `data-color-scheme` lifecycle.
- [ ] Replace built-in result glyphs with Lucide file-search, calculator, and browser-search compositions while preserving the accepted semantic color classes and stable result-icon dimensions.
- [ ] Replace public-plugin install, folder, save, reset, delete, and related host action glyphs with Lucide equivalents; retain all tooltips, labels, confirmation flows, loading states, and client calls.
- [ ] Apply shared tokens to the launcher, results, Settings header/tabs/forms, public-plugin sections, confirmations, selected states, scrollbars, and status text without adding cards or changing density.
- [ ] Keep the working OverlayScrollbars Ant body-height selectors and the empty legacy-plugin suppression intact.

**Distinct test coverage:** Root and surface schemes remain synchronized; shared theme config replaces local construction; every in-scope host icon is Lucide with accessible naming; colored built-in icons and PNG/fallback behavior remain; General and Plugins scrolling, settings persistence UI, public-plugin operations, and no-close-on-mutation behavior remain green.

**Verify:** `npm.cmd test -- --run src/launcher.test.tsx`

### Task 5: Dependency Cleanup And Integration Verification

**Files:** Modify `package.json`, `package-lock.json`, `src/styles.css`; update tests only where final dependency assertions belong.

**Dependencies:** Tasks 1-4; approved design sections `Migration Order`, `Testing`, `Manual Acceptance`, and `Acceptance Criteria`.

- [ ] Confirm the repository contains no `@ant-design/icons` imports, then remove the dependency; if any out-of-scope import remains, keep the dependency and record the exact remaining file instead of breaking the build.
- [ ] Remove superseded generic hard-coded surface/control palette rules while retaining semantic built-in icon colors, forced-colors rules, stable dimensions, drag regions, and responsive constraints.
- [ ] Verify no Tailwind/Radix package or runtime icon download was introduced and plugin-facing variables remain stable.
- [ ] Run the complete automated gate and prepare the four-surface light/dark manual checklist without launching or focusing real windows.

**Distinct test coverage:** Dependency/import consistency; single generic host palette; plugin compatibility variable names; complete frontend regression suite; production TypeScript/Vite build.

**Verify:** `npm.cmd test -- --run` then `npm.cmd run build` and `git diff --check`

## Final Checklist

- [ ] All five task commits contain only their scoped changes plus approved review fixes.
- [ ] Full frontend tests and production build pass.
- [ ] Main, Settings, `/find`, and plugin host shell consume one semantic theme layer in light and dark modes.
- [ ] Existing runtime, focus, pin, close, scrolling, and plugin behaviors are unchanged.
- [ ] User is notified to perform the approved `Manual Acceptance` steps; no automated input or foreground control is used.
