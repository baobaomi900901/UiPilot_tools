# Switch Checked Color Implementation Plan

**Goal:** Make every host Ant Design `Switch` green when checked without changing global accent colors or unchecked styling.

**Architecture:** Keep the change inside the existing `uiThemeConfig` boundary. A component-scoped Ant Design `Switch` token override supplies the checked and checked-hover colors to every current Switch consumer in launcher, Find, and public-plugin views.

**Tech Stack:** TypeScript, Ant Design theme tokens, Vitest, Vite.

**Approved specification:** [`docs/superpowers/specs/2026-08-24-switch-checked-color-design.md`](../specs/2026-08-24-switch-checked-color-design.md)

## Global Constraints

- Checked track is `#16a34a`; checked hover track is `#15803d` in light and dark themes.
- Do not change global `colorPrimary`, semantic tokens, individual Switch call sites, or CSS selectors.
- Preserve all pre-existing user worktree changes.
- Use a focused failing theme test before implementation; commit only this task's files.
- No real window, foreground focus, mouse, or keyboard interaction is required.

### Task 1: Component-Scoped Switch Theme

**Files:** `src/ui-theme.ts`, `src/ui-theme.test.ts`

**Dependencies:** Design sections `Goal`, `Design`, and `Verification`.

- [ ] Extend the focused Ant theme test to require `components.Switch.colorPrimary === '#16a34a'` and `colorPrimaryHover === '#15803d'` for both schemes, while retaining the existing global-primary assertions.
- [ ] Add only the component-scoped `Switch` tokens to `uiThemeConfig`.
- [ ] Confirm existing theme projections and dependencies remain unchanged.

**Distinct test coverage:** Light and dark configurations expose the same green checked/hover tokens, while their global `token.colorPrimary` remains the existing semantic primary.

**Verify:** `npm.cmd test -- src/ui-theme.test.ts --run`; `npm.cmd run build`

## Final Checklist

- [ ] Focused theme test passes.
- [ ] Production frontend build passes.
- [ ] No unrelated worktree changes are staged or committed.
