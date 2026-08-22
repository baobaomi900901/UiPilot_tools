# Public Plugin Validation CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver `@uipilot/plugin-cli` so third-party developers can validate UiPilot public-plugin directories and `.uipilot-plugin` archives using only Node.js 20+.

**Architecture:** A bundled TypeScript CLI maps directory and ZIP inputs into one bounded `PackageSnapshot`, then runs deterministic Manifest, compatibility, resource, PNG, CSS, and WAV validators and renders human or stable JSON reports. The approved [design specification](../specs/2026-08-20-public-plugin-validation-cli-design.md) governs all contracts, with the current host Timer API and alarm-resource rules in `docs/plugin-sdk/public-plugin-v1.md` taking precedence over the older permission list in the specification.

**Tech Stack:** Node.js 20+, TypeScript, npm workspaces, Vitest, Ajv standalone generation, esbuild, pngjs, pinned Unicode 15.1 data.

## Global Constraints

- Validate only; do not install, execute plugin code, contact UiPilot, modify the source, publish npm artifacts, or add `pack`.
- Preserve stable report DTOs, issue codes, ordering, 100-issue cap, and exit codes from the approved specification.
- Runtime artifacts are self-contained ESM with no runtime dependencies, network path, Ajv compiler, `eval`, or `Function`.
- `timer.control` is valid only for Windows with `submit + window + window entry + ui.window + notifications.publish`.
- A `timer.control` package must contain exactly `assets/sounds/timer-alarm.wav`; packages without that permission must not contain it, and every other WAV path is invalid.
- Alarm WAV must exactly match the current host parser: RIFF/WAVE, canonical `fmt ` then `data`, PCM 1, 1/2 channels, 44.1/48 kHz, 16/24-bit little-endian samples, at most 2 MiB and 15 seconds, with no unknown/duplicate chunks or trailing bytes.
- Work in the current branch as authorized. Do not modify or commit pre-existing user changes, create review agents, push, or perform GUI/input tests.

## Global Execution Rules

- Dependency order is `Task 1 -> Task 2 -> Task 3 -> Task 4 -> Task 5`.
- Each task uses focused test-first cycles and produces one atomic commit containing only that task's files.
- Generated files are verified by check mode; source data and build scripts remain pinned and reproducible.
- No real window, focus, mouse, or keyboard operations are required.

### Task 1: Workspace, CLI Contract, And Deterministic Reports

**Files:** root `package.json`, `package-lock.json`; create `packages/plugin-cli/package.json`, `tsconfig.json`, `src/cli.ts`, `src/contracts.ts`, `src/report.ts`, `src/index.ts`, and focused tests.

**Dependencies:** Design sections `Distribution And Compatibility`, `Command Contract`, `JSON Output`, `Stable Issue Codes`, and `Failure Behavior`.

- [ ] Add the npm workspace, Node 20 engine, `uipilot-plugin` bin, build/test/check scripts, and publish allowlist.
- [ ] Implement argument parsing, platform defaulting, exit-code mapping, human output, exact JSON DTO output, total issue ordering, deduplication, cap, and `truncated` behavior.
- [ ] Keep usage/internal errors separate from validation reports and suppress progress output in JSON mode.

**Distinct test coverage:** valid/invalid syntax; unsupported host default; source failures remain exit 1; deterministic top-100 ordering and truncation; JSON mode emits exactly one DTO.

**Verify:** `npm test --workspace @uipilot/plugin-cli -- cli.test.ts report.test.ts`

### Task 2: Safe Directory And ZIP Package Snapshots

**Files:** create `src/package-policy.ts`, `src/unicode.ts`, generated Unicode data and generator, `src/directory-reader.ts`, `src/windows-attributes.ts`, `src/archive-reader.ts`, `src/crc32.ts`, and focused tests.

**Dependencies:** Task 1; design sections `Source Readers`, `Package Policy`, `Package Validation Rules`, and `Data Flow`.

- [ ] Implement Unicode 15.1 NFC/full-fold/NFC identity, canonical path/resource rules, bounds, reserved names, collision checks, and immutable snapshots.
- [ ] Implement no-follow directory traversal and the fail-closed Windows attribute adapter with fixed absolute PowerShell, bounded UTF-8 protocol, pre/post checks, and no source-controlled process lookup.
- [ ] Implement bounded raw ZIP central/local parsing for Stored/Deflate, creator/external type checks, streaming size/CRC verification, duplicate/collision checks, and no extraction.

**Distinct test coverage:** Unicode recomposition/Hangul/full-fold collisions; traversal and limits; fake local PowerShell cannot run; Windows protocol CJK/non-BMP; archive links/devices, bad CRC for both methods, malformed headers, encryption, unsupported compression, and decompression bounds.

**Verify:** `npm test --workspace @uipilot/plugin-cli -- package-policy.test.ts directory-reader.test.ts archive-reader.test.ts`

### Task 3: Strict Manifest, Schema, And Current Permission Semantics

**Files:** create `src/strict-json.ts`, `src/manifest.ts`, `scripts/generate-schema-validator.mjs`, generated standalone validator, copied pinned Schema input, and focused tests.

**Dependencies:** Task 2; design sections `Manifest Validation` and `Package Validation Rules`; current `docs/plugin-sdk/uipilot-plugin-v1.schema.json` and `docs/plugin-sdk/public-plugin-v1.md`.

- [ ] Fatal-decode UTF-8 and parse strict JSON with duplicate-key, scalar-surrogate, finite-number, and depth enforcement before Schema validation.
- [ ] Generate and check an Ajv strict standalone ESM validator with inline `uint32` and `double` formats and no runtime Ajv compiler.
- [ ] Enforce canonical identities/versions/settings/entries, selected platform, API/host compatibility, and current permission/output relationships.
- [ ] Accept `timer.control` only for the complete Windows `submit + window + window entry + ui.window + notifications.publish` combination and reject it for macOS or incomplete combinations.

**Distinct test coverage:** invalid UTF-8, duplicate nested keys, isolated/reversed surrogates, Schema diagnostics, setting boundaries, version/API/platform failures, and a table of every valid/invalid `timer.control` combination.

**Verify:** `npm test --workspace @uipilot/plugin-cli -- strict-json.test.ts manifest.test.ts`

### Task 4: PNG, CSS, And Timer Alarm Resource Validation

**Files:** create `src/png-validator.ts`, `src/css-validator.ts`, `src/wav-validator.ts`, `src/validate.ts`, and focused tests.

**Dependencies:** Task 3; design sections `Specialized Validators` and `Data Flow`; current host `alarm_asset.rs` behavior recorded in the SDK contract.

- [ ] Validate raw PNG chunk framing/CRC/IEND/APNG rules before complete pngjs decoding and exact icon size limits.
- [ ] Reproduce the host's strict UTF-8 ASCII-insensitive CSS scanner and package-local reference resolution.
- [ ] Enforce the fixed timer alarm path/permission bijection, reject every other WAV, and implement the exact canonical PCM parser and bounds.
- [ ] Compose validator phases with the frozen skip/continue behavior and stable report production.

**Distinct test coverage:** PNG CRC/order/trailing/APNG/dimensions; CSS comments/strings/whitespace/media/nesting compatibility; WAV channel/rate/bit-depth/byte-rate/block-align/duration/size/padding/chunk/order/trailing failures; missing/extra/undeclared alarm resources; pomodoro passes on Windows and fails on macOS.

**Verify:** `npm test --workspace @uipilot/plugin-cli -- png-validator.test.ts css-validator.test.ts wav-validator.test.ts validate.test.ts`

### Task 5: Bundled Artifact, Offline Smoke, And Developer Documentation

**Files:** create build/audit/smoke scripts, `packages/plugin-cli/README.md`, `LICENSE`, `THIRD_PARTY_NOTICES`; modify `docs/plugin-sdk/public-plugin-developer-guide.md`; add artifact tests.

**Dependencies:** Task 4; design sections `Artifact Smoke Test`, `Documentation`, and `Acceptance Criteria`.

- [ ] Bundle the executable, generated validator, and Unicode data; audit allowed built-ins, the sole Windows child-process callsite, absence of network/compiler/dynamic-code capabilities, and npm tarball contents.
- [ ] Install only the `.tgz` offline in a repository-external temporary project with empty cache and run directory/archive validation using filesystem restrictions and network API traps.
- [ ] Document local/CI validation commands, platform behavior, human/JSON reports, exit codes, Timer/WAV failures, and the no-install/no-execution boundary.

**Distinct test coverage:** bin mapping, valid demos including pomodoro, invalid Timer/WAV fixture, no parent dependency/Schema lookup, one valid and invalid standalone Schema call without Ajv, and exact tarball allowlist.

**Verify:** `npm run verify --workspace @uipilot/plugin-cli`

## Final Checklist

- [ ] Focused tests and full workspace tests pass.
- [ ] TypeScript check, bundle audit, packed-artifact smoke, and root build pass.
- [ ] Current `demo-return`, `demo-win`, and Windows pomodoro directories validate; pomodoro is rejected for macOS.
- [ ] The source packages remain unchanged and no GUI/input operation occurred.
- [ ] Only CLI/plan/developer-guide files are included in task commits; existing user changes remain untouched.
