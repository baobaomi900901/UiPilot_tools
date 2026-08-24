# Demo Plugin Examples Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Completely remove installed public-plugin data and replace the combined `/demo` fixture with independent `/demo-win` and `/demo-return` examples.

**Architecture:** Each example is a static package with its own identity, manifest, runtime, documentation, and tests. A single packaging script accepts one of the two plugin IDs, while the host package validator remains the authority for permissions, assets, and response routing.

**Tech Stack:** Tauri 2 / Rust, JavaScript ES modules, TypeScript contract checks, PowerShell packaging.

**Specification:** [`docs/superpowers/specs/2026-08-17-demo-plugin-examples-design.md`](../specs/2026-08-17-demo-plugin-examples-design.md)

## Global Constraints

- Never terminate UiPilot or synthesize mouse or keyboard input; installed-data cleanup waits for the user to close the running process.
- Delete only `%APPDATA%\com.uipilot.launcher\public-plugins`; preserve repository sources and built-in launcher features.
- `/demo-win` is permanently `submit + window` with only `ui.window`.
- `/demo-return` is permanently `submit + mainResult` with only `clipboard.write` and no window assets.
- Preserve all unrelated working-tree changes; commits may contain only isolated task files or hunks.

## Global Execution Rules

- Dependency order: `Task 1 -> Task 2 -> Task 3 -> Task 4`.
- Source tasks use focused tests before and after implementation, then run the final combined verification once.
- Do not start UiPilot or any real-window harness. Manual installation and UI acceptance belong to the user.

---

### Task 1: Remove Installed Public-Plugin Data

**Files:** system data root `%APPDATA%\com.uipilot.launcher\public-plugins` only.

**Dependencies:** design sections `Installed Data Cleanup` and `Acceptance`.

- [x] Confirm no `uipilot` process is running; otherwise ask the user to close it and stop this task.
- [x] Resolve and verify the absolute target remains exactly under `%APPDATA%\com.uipilot.launcher`.
- [x] Recursively delete the `public-plugins` root, including packages, state, settings, storage, secrets, and staging.

**Distinct verification:** the target root no longer exists; repository example directories remain present.

**Verify:** `Test-Path -LiteralPath "$env:APPDATA\com.uipilot.launcher\public-plugins"`

### Task 2: Convert The Existing Fixture To `/demo-win`

**Files:** move `examples/public-plugins/com.uipilot.demo` to `examples/public-plugins/com.uipilot.demo-win`; modify its `README.md`, `package/plugin.json`, `package/dist/runtime.js`, `tests/runtime.test.js`, and `tests/sdk-contract.ts`.

**Dependencies:** Task 1; design section `` `/demo-win` ``.

- [x] Rename the example directory, plugin ID, display name, command name, test request IDs, and documentation to `demo-win` / `com.uipilot.demo-win`.
- [x] Remove the runtime output-mode constant and main-result branch; always return `{ requestId, data: { returnText } }`.
- [x] Keep exactly the four existing window assets, singleton update bridge assertions, and `ui.window` permission.

**Distinct test coverage:** the strict package root has five resources; the Runtime returns `str yyyy-mm-dd` as window data; the window bridge still exposes the five acceptance fields and no privileged APIs.

**Verify:** `node --test examples/public-plugins/com.uipilot.demo-win/tests/runtime.test.js` and `npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.demo-win/tests/sdk-contract.ts`

### Task 3: Create `/demo-return`

**Files:** create `examples/public-plugins/com.uipilot.demo-return/README.md`, `package/plugin.json`, `package/dist/runtime.js`, `tests/runtime.test.js`, and `tests/sdk-contract.ts`.

**Dependencies:** Task 2; design section `` `/demo-return` ``.

- [x] Create a strict package containing only `plugin.json` and `dist/runtime.js`, with ID `com.uipilot.demo-return`, command `demo-return`, `submit + mainResult`, and `clipboard.write`.
- [x] Implement an asynchronous `onCommand` that returns one result titled `input + local yyyy-mm-dd` with the same text in its `copyText` default action.
- [x] Document first-Enter publication, default selection, second-Enter copy, and development-directory installation.

**Distinct test coverage:** the package contains no window member or assets; the Runtime preserves spaces inside the input, binds the exact request ID, and emits exactly one copy action.

**Verify:** `node --test examples/public-plugins/com.uipilot.demo-return/tests/runtime.test.js` and `npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.demo-return/tests/sdk-contract.ts`

### Task 4: Update Packaging, SDK References, And Host Fixtures

**Files:** modify `scripts/package-demo-plugin.ps1`, `docs/plugin-sdk/public-plugin-v1.md`, and `src-tauri/src/public_plugins/tests.rs`.

**Dependencies:** Tasks 2 and 3; design sections `Source And Packaging`, `Validation`, and `Acceptance`.

- [x] Parameterize the packaging script for the exact allowlist `com.uipilot.demo-win` and `com.uipilot.demo-return`; resolve each source root independently and preserve atomic archive replacement.
- [x] Point current SDK documentation at both examples and explain their fixed output modes.
- [x] Replace the single repository fixture test with assertions that both packages stage with exact IDs, commands, modes, permissions, window presence, and resource counts.
- [x] Package and stage both archives through the shared script.

**Distinct test coverage:** the window archive has five package resources and `ui.window`; the return archive has two package resources (`plugin.json` plus one Runtime) and `clipboard.write`; production host code contains neither literal command.

**Verify:** `cargo test public_plugins::tests::repository_demo` and `cargo test public_plugins::tests::demo_packaging`

## Final Checklist

- [x] Both focused JavaScript and TypeScript suites pass.
- [x] Both development directories and generated archives pass the Rust package validator.
- [x] `cargo test` passes.
- [x] No GUI process or input automation was started.
- [ ] User manually installs and accepts `/demo-win` and `/demo-return`.
