# Public Plugin PNG Icon Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add one optional fixed-root `icon.png` identity asset to public plugins and render it safely across host-controlled plugin surfaces.

**Architecture:** Package staging fully decodes and validates the one allowed PNG before atomic commit. Existing immutable activation snapshots expose only generation- or prepare-token-bound URLs through a narrow custom-protocol branch; shared frontend rendering consumes host-owned URL fields and falls back locally without giving Runtime control of icons.

**Tech Stack:** Rust 1.96, `png` 0.18 (already present transitively), Tauri 2.11, TypeScript 7, React 19, Ant Design 6, Lucide React, Vitest 4.

**Approved specification:** [`docs/superpowers/specs/2026-08-18-public-plugin-png-icon-design.md`](../specs/2026-08-18-public-plugin-png-icon-design.md), especially `Package Contract`, `Host Icon URLs`, `Wire Contracts`, `UI Rendering`, and `Failure Behavior`.

## Global Constraints

- The only custom icon path is optional package-root `icon.png`; `plugin.json` and schema version 1 do not gain an icon field.
- Accept only a completely decodable static 128 by 128 PNG no larger than 128 KiB; reject every other PNG path and APNG.
- Invalid fresh installs persist nothing; invalid updates preserve the active generation, Runtime, settings, permissions, and prior icon.
- Main and the matching plugin shell receive only host-generated URLs. Runtime/content/other labels, stale generations, and retired prepare tokens fail closed before asset bytes are read.
- Runtime response DTOs cannot provide per-result icons.
- Keep shell pin, close, focus, singleton, launcher sizing, and result execution behavior unchanged.
- Do not synthesize input or control the user's mouse or keyboard. Real-window acceptance waits for explicit user action.
- Preserve all pre-existing worktree changes. Commit only task-owned hunks that can be isolated; never absorb unrelated changes.

## Shared Contracts

Use the exact DTOs and URL ownership rules in the specification's `Host Icon URLs` and `Wire Contracts` sections. Centralize fixed path, byte/dimension/animation validation, URL construction/parsing, and frontend URL validation rather than duplicating string rules across callers.

## Global Execution Rules

- Dependency order is `Task 1 -> Task 2 -> Task 3`.
- Each task follows focused TDD: add a failing risk-focused test, confirm the intended failure, implement the minimum contract, and rerun focused verification.
- Run full suites only in final verification. Do not add a separate test for every equivalent API method or UI size.
- Keep commits task-scoped when hunks can be isolated from the dirty worktree; otherwise leave verified implementation unstaged and report the boundary.

---

### Task 1: Fixed PNG Package Resource

**Files:** `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/src/public_plugins/icon.rs` (new), `src-tauri/src/public_plugins.rs`, `src-tauri/src/public_plugins/package.rs`, `src-tauri/src/public_plugins/tests.rs`, `docs/plugin-sdk/public-plugin-v1.md`, `examples/public-plugins/com.uipilot.demo-win/package/plugin.json`, `examples/public-plugins/com.uipilot.demo-return/package/plugin.json`, `scripts/package-demo-plugin.ps1`

**Dependencies:** Approved design sections `Package Contract`, `Validation And Atomicity`, `Demo Packages`, and `Failure Behavior`.

- [ ] Add a focused icon module with `ICON_PATH`, `ICON_MIME`, `MAX_ICON_BYTES`, and full bounded PNG decoding that accepts only static 128 by 128 content.
- [ ] Let enumeration classify lowercase PNG as `image/png`, then reject unless the entire PNG resource set is empty or exactly root `icon.png` and its bytes pass the icon module.
- [ ] Preserve existing snapshot hashing/revalidation/read-only behavior for the accepted PNG and keep CSS/content image permissions unchanged.
- [ ] Document the convention, advance both demos to `1.0.2`, and update staging/packaging resource counts to include the user-supplied root PNGs.

**Distinct test coverage:** absent icon succeeds; exact valid icon succeeds; one table rejects corrupt, truncated, wrong-size, APNG, oversized, wrong-case/path, and additional PNG packages while leaving staging empty.

**Verify:** `cargo test public_plugin_icon`; `cargo test repository_demo_examples_stage_as_independently_removable_public_plugins`; `cargo test demo_packaging_script_writes_both_installable_archives` from `src-tauri`.

### Task 2: Host URL, DTO, And Caller Authorization

**Files:** `src-tauri/src/public_plugins/icon.rs`, `src-tauri/src/public_plugins/activation.rs`, `src-tauri/src/public_plugins.rs`, `src-tauri/src/model.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/plugin_window.rs`, `src-tauri/src/lib.rs`, `src-tauri/capabilities/plugin-window-shell.json`, generated permission metadata as required

**Dependencies:** Task 1; approved design sections `Host Icon URLs`, `Wire Contracts`, `Data Flow`, and `Failure Behavior`.

- [ ] Add nullable inventory/prepare icon URLs, optional host-only result icon URLs, and read-only plugin-window identity while keeping Runtime response types icon-free.
- [ ] Construct installed URLs from current `pluginId + generation` and prepared URLs from the live caller-bound token; include icon identity in suggestions, public main results, inventory, and prepare summaries.
- [ ] Extend the existing custom protocol with exact installed/prepared icon routes. Authorize current `main` or the matching shell only, retire staged URLs on commit/cancel/expiry, deny stale/uninstalled identities, and apply immutable versus `no-store` cache headers as specified.
- [ ] Add a shell-only identity command that derives plugin identity from the exact caller label before manager access and returns only manifest name plus current icon URL.
- [ ] Wire the command and narrow shell capability without broadening Runtime, content, find, or main command permissions.

**Distinct test coverage:** current main and matching shell resolve bytes; Runtime/content/unrelated shell/find labels are rejected before byte access; old generation, uninstall, expired/canceled/committed prepare token fail; successful update changes URL while failed update retains the active generation and icon; Runtime results cannot inject an icon; exact DTO serialization contains only host URL fields.

**Verify:** `cargo test public_plugin_icon`; `cargo test public_plugin_window_identity`; `cargo test public_plugin_command_discovery`; `cargo test public_plugin_commands_have_non_overlapping_exact_capabilities` from `src-tauri`.

### Task 3: Shared Rendering Across Host Surfaces

**Files:** `src/plugin-icon.tsx` (new), `src/protocol.ts`, `src/main.ts`, `src/launcher-core.ts`, `src/launcher-view.tsx`, `src/public-plugin-panel.tsx`, `src/plugin-window-core.ts`, `src/plugin-window-view.tsx`, `src/styles.css`, `src/launcher.test.tsx`, `src/plugin-window-view.test.tsx`

**Dependencies:** Task 2; approved design sections `Wire Contracts`, `UI Rendering`, `Data Flow`, and `Acceptance`.

- [ ] Add strict frontend host-icon URL validation and a shared noninteractive `PluginIcon` component with fixed size variants, theme-token neutral tile, Lucide fallback, empty alt text, and image-error fallback.
- [ ] Parse nullable inventory/prepare URLs exactly, project result `pluginIconUrl` through launcher ownership, and render the shared component in command suggestions, plugin main results, install confirmation, and settings rows.
- [ ] Load the shell's caller-derived identity through `PluginWindowCore`, render 20-pixel icon plus manifest name instead of fixed `UiPilot`, and preserve pin/close pending and error state.
- [ ] Keep ordinary app data icons, built-in icons, accessibility selection, list sizing, and non-plugin rows unchanged.

**Distinct test coverage:** malformed icon URLs fall back and cannot navigate; command and public-result rows render 28-pixel icons; install preview and settings use 32/36 pixels; shell identity uses 20 pixels and manifest name; missing/load-error paths use one default glyph; existing launcher height and shell pin/close behavior remain green.

**Verify:** `npm test -- src/launcher.test.tsx src/plugin-window-view.test.tsx`; `npm run build`.

## Final Verification And Acceptance

- [ ] Run `cargo fmt --check`, `cargo test`, and `cargo clippy --all-targets --no-default-features -- -D warnings` from `src-tauri`.
- [ ] Run `npm test` and `npm run build` from the repository root.
- [ ] Run `git diff --check` and inspect task-owned hunks without staging pre-existing user changes.
- [ ] Ask the user to upgrade/reinstall both `1.0.2` demos, then manually verify install preview, settings, `/d`, `demo-return`, and `demo-win` header in Dark and Light. Do not control input or foreground focus.
