# Find contextual actions

Goal: use the shared Ctrl+K menu in the host-owned /find window, following the user-specified focus-dependent actions.
Architecture: reuse context-actions-ui; FindCore remains the owner of searching, pinning, preview preferences and authenticated file reveal. No new backend APIs. React/TypeScript; keep existing worktree edits and native keyboard/IME boundaries.

### Task 1: Search vs result activation
- [x] Add an explicit submitSearch entry point; search-input Enter must search without executing a selected result.
- [x] Arrow navigation and pointer selection focus actual result rows; Enter on a row preserves authenticated folder reveal.
Verify: npm test -- src/find-core.test.ts src/find-view.test.tsx

### Task 2: Shared menu and focus ownership
- [x] Add right-aligned Ctrl+K footer entry; global pin toggle Ctrl+T, preview toggle, return-to-search Ctrl+F.
- [x] Context action: search Enter for input; reveal containing folder Enter for a selected result. No result -> disabled explanation.
- [x] Capture focus before opening; ignore IME/repeat; Escape closes menu only. Menu arrows cannot navigate background results.
- [x] Invalidate callbacks on query/invocation/category/selection/search or action-state changes and unmount. Thumbnail-only updates should not close the menu.
Verify: targeted view tests including menu execution, empty state, newer search while menu open, pin/preview toggles and Escape.

### Task 3: Verification
- [x] Production build, focused tests and synthetic browser preview in dark/light. No real file execution or preference modification during testing.
- [x] Report manual acceptance steps for live /find.

Verification: 65 focused tests passed; production build passed (existing bundle-size warning); dark/light synthetic browser previews verified. Native file-manager reveal remains a manual acceptance check. No commits created; existing worktree changes preserved.
