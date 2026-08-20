# Public Plugin Validation CLI Design

**Status:** Draft - user-approved design sections, awaiting written-spec review

**Date:** 2026-08-20

## Goal

Provide third-party plugin developers with a standalone npm CLI that validates a UiPilot public plugin development directory or `.uipilot-plugin` archive without requiring Rust, an UiPilot installation, or the UiPilot source tree.

The first release is distributed as `@uipilot/plugin-cli`, requires Node.js 20 or newer, and exposes one command:

```text
npx @uipilot/plugin-cli validate <source> [--platform windows|macos] [--json]
```

The CLI validates packages only. It never installs a plugin, executes plugin code, contacts a running UiPilot process, or modifies the source package.

## Scope

### Included

- Development-directory and `.uipilot-plugin` archive validation.
- Human-readable and machine-readable output.
- Windows and macOS target selection.
- Package structure, limits, paths, resources, Manifest, entry files, icon, CSS references, platform, API, and permission validation.
- A publishable npm package plus a packed-artifact smoke test.
- Developer-guide examples for local use and CI.

### Excluded

- Publishing `@uipilot/plugin-cli` to the npm registry.
- Plugin packaging; the later `pack` task adds that subcommand.
- Executing Runtime or window JavaScript.
- Installing a package into UiPilot.
- Network access, update checks, or telemetry.
- The complete cross-language conformance corpus; the later contract-test task adds it.

## Distribution And Compatibility

The repository becomes an npm workspace with `packages/plugin-cli` as an independently publishable package. The package starts at version `0.1.0` and declares `node >=20`.

CLI release version and host compatibility are separate. This release validates against:

- UiPilot host version `0.2.0`.
- Public Plugin API version `1`.
- Package Schema version `1`.

The executable name is `uipilot-plugin`. The CLI uses a subcommand-oriented command parser so the later packaging task can add `pack` without changing the `validate` contract.

The npm artifact contains every runtime dependency, the UiPilot Plugin v1 JSON Schema, and generated Unicode folding data. It does not read files outside the selected package except its own bundled assets.

## Command Contract

### Syntax

```text
uipilot-plugin validate <source> [--platform <platform>] [--json]
```

`source` must identify exactly one of:

- An ordinary directory containing `plugin.json` at its root.
- An ordinary file whose extension is exactly `.uipilot-plugin`.

`--platform` accepts `windows` or `macos`. If omitted, Windows defaults to `windows`, macOS defaults to `macos`, and any other operating system returns a usage error requiring an explicit platform.

`--json` selects the stable machine-readable response. The default response is concise English text for a developer. Validation does not prompt for input.

### Exit Codes

- `0`: the package is valid for the selected target.
- `1`: the source is unsafe, unreadable, invalid, or incompatible.
- `2`: command usage is invalid or the CLI itself cannot run.

Expected filesystem errors while reading the selected source are validation failures and use exit code `1`. An unexpected CLI defect uses `CLI_INTERNAL` and exit code `2`.

## Architecture

### CLI Adapter

The CLI adapter parses arguments, resolves the default platform, invokes the validation core, selects the renderer, and maps the result to an exit code. It catches all errors at the process boundary. JSON mode writes exactly one JSON document to standard output and no progress text.

### Source Readers

Directory and ZIP readers produce the same bounded in-memory `PackageSnapshot`. A snapshot contains canonical package-relative paths, resource bytes, resource metadata, and directory identities needed for collision checks.

The directory reader uses no-follow metadata and rejects symbolic links, junctions, reparse points, sockets, devices, and any entry that is not an ordinary file or directory.

The archive reader processes the central directory lazily. It reads raw names, requires valid UTF-8, rejects encrypted or unsupported compression entries, checks declared and actual uncompressed sizes, and never extracts the archive to disk. Only Stored and Deflate compression are accepted.

Unsafe source acquisition is fail-fast. Once a safe bounded snapshot exists, independent validators may collect multiple issues.

### Package Policy

The package-policy module owns the host-equivalent constants and pure path/resource rules. It applies Unicode 15.1 `NFC -> full case fold -> NFC` when finding case-insensitive collisions. Unicode data is generated at build time from `@unicode/unicode-15.1.0` and bundled as a compact runtime table.

The source tree and archive share the same canonical-path and resource checks. Path identity never relies on locale-sensitive lowercasing.

### Manifest Validation

The bundled JSON Schema is compiled once with Ajv. JSON parsing and Schema failures remain distinct. After Schema validation, semantic validation enforces host contracts that JSON Schema alone cannot express, including output-mode/window/permission relationships, canonical versions, target-platform compatibility, API compatibility, setting defaults, unique fields, and platform-specific permissions.

### Specialized Validators

- The entry validator requires the declared Runtime and optional window entry to exist with the exact allowed extension.
- The icon validator uses `pngjs` for complete CRC-checked decoding, requires `128 x 128`, rejects APNG animation chunks, and applies the byte limit.
- The CSS validator recognizes local `url(...)` and `@import` references and resolves them only within the package snapshot.
- The report builder caps results at 100 issues and sorts them deterministically by package-relative path and then error code.

No validator evaluates HTML, CSS, or JavaScript.

## Package Validation Rules

The CLI matches the UiPilot `0.2.0` host rules:

- At most 64 directories and 256 files.
- At most 8 path components.
- At most 2 MiB per file and 16 MiB total uncompressed file content.
- At most 240 UTF-8 bytes per path and 100 UTF-8 bytes per component.
- Archive file size at most 16 MiB and no more than 320 central-directory entries.
- Paths must be relative NFC text using `/` separators.
- Empty components, `.`, `..`, trailing dot/space, control characters, Windows-forbidden characters, Windows reserved names, and full-fold collisions are rejected.
- `plugin.json` is required at the archive or directory root.
- Resource basenames have exactly one extension.
- Allowed extensions are `.html`, `.js`, `.css`, and `.png`; the only PNG resource is optional root `icon.png`.
- `icon.png` is at most 128 KiB, exactly `128 x 128`, fully decodable, CRC-valid, and non-animated.
- CSS references are non-empty package-local paths without protocols, absolute paths, backslashes, query strings, fragments, percent encoding, or control characters.
- Referenced CSS resources must exist after canonical relative resolution.
- Manifest `supportedPlatforms` must contain the selected target.
- Manifest API version must equal `1` and `minimumHostVersion` must not exceed `0.2.0`.
- Only permissions implemented by the selected target are accepted. In the current host contract, `ui.window` and `clipboard.write` are available on both targets, while `notifications.publish` is available only on Windows; other declared permissions are rejected.

## Data Flow

1. Parse arguments and choose target platform.
2. Classify the source as directory or archive without following links.
3. Acquire a bounded, canonical package snapshot.
4. Parse and Schema-validate `plugin.json`.
5. Run Manifest semantic and compatibility validation.
6. Validate declared entries, resources, icon, and CSS references.
7. Sort and cap issues, render the selected output, and return the fixed exit code.

If safe snapshot acquisition fails, later steps do not run. A validator may not turn a prior fatal source error into a partial success.

## JSON Output

Validation results use this DTO:

```ts
interface PluginValidationReportV1 {
  schemaVersion: 1;
  valid: boolean;
  source: {
    kind: "directory" | "archive";
    path: string;
  };
  target: {
    platform: "windows" | "macos";
    hostVersion: "0.2.0";
    apiVersion: 1;
  };
  plugin?: {
    pluginId: string;
    version: string;
    outputMode: "mainResult" | "window";
  };
  issues: Array<{
    code: PluginValidationIssueCode;
    path?: string;
    message: string;
  }>;
}
```

`source.path` preserves the source argument supplied by the caller. Issue paths always use canonical package-relative `/` paths. The optional plugin summary appears only after enough Manifest fields have passed structural validation; it is informational and does not imply validity.

Usage and internal failures use a separate DTO:

```ts
interface PluginCliErrorV1 {
  schemaVersion: 1;
  error: {
    code: "CLI_USAGE" | "CLI_INTERNAL";
    message: string;
  };
}
```

The CLI never includes source contents, plugin storage, secrets, environment variables, or stack traces in either DTO.

## Stable Issue Codes

`PluginValidationIssueCode` contains:

- `SOURCE_INVALID`
- `ARCHIVE_INVALID`
- `PACKAGE_LIMIT_EXCEEDED`
- `PATH_INVALID`
- `PATH_COLLISION`
- `RESOURCE_INVALID`
- `MANIFEST_MISSING`
- `MANIFEST_JSON_INVALID`
- `MANIFEST_SCHEMA_INVALID`
- `MANIFEST_SEMANTIC_INVALID`
- `PLATFORM_INCOMPATIBLE`
- `API_INCOMPATIBLE`
- `PERMISSION_UNSUPPORTED`
- `ENTRY_MISSING`
- `ICON_INVALID`
- `CSS_REFERENCE_INVALID`

Codes and DTO fields are compatibility contracts. Messages are concise English diagnostics and may improve without a major CLI version change.

## Failure Behavior

- Source and archive errors do not create files or modify the source.
- Package limits are checked with overflow-safe arithmetic before allocating or appending data.
- The CLI closes every file and ZIP handle on success and failure.
- A malformed ZIP, JSON document, PNG, CSS reference, or Unicode path becomes a bounded validation issue rather than an uncaught exception.
- More than 100 reportable issues produces the first 100 deterministic issues plus no unbounded detail.
- JSON mode remains valid JSON for all expected exit paths.
- Internal exceptions return the fixed `CLI_INTERNAL` response without a stack trace.

## Testing

### Unit Tests

- Argument parsing, platform defaulting, output rendering, and exit-code mapping.
- Canonical path validation, UTF-8 byte limits, Windows reserved names, NFC enforcement, and Unicode 15.1 full-fold collisions.
- Manifest Schema and semantic validation.
- CSS reference parsing and package-bound resolution.
- PNG byte, dimension, CRC, decode, and animation behavior.
- Deterministic sorting and the 100-issue cap.

### Integration Tests

- Valid directory packages for `mainResult` and `window` modes.
- Valid Stored and Deflate archives.
- Encrypted, malformed, traversal, duplicate, case-fold-collision, unsupported-resource, oversized, and missing-entry packages.
- Platform, API, and permission incompatibility.
- Both human and JSON command output with exact exit codes.
- Repository `demo-win` and `demo-return` packages.

### Artifact Smoke Test

The build creates an npm `.tgz`, initializes a fresh temporary npm project outside `packages/plugin-cli`, installs only that tarball, and invokes `node_modules/.bin/uipilot-plugin`. The smoke test validates one directory and one archive and proves the installed CLI does not resolve code or Schema files from the UiPilot repository.

### Deferred Conformance Work

This task includes only the minimum shared cases needed to prove the implementation. The later contract-test task creates a larger fixture corpus consumed by both the Rust host validator and this TypeScript CLI, including fixed Unicode, ZIP, Manifest, icon, and CSS vectors.

## Documentation

The public plugin developer guide will document:

- Node.js 20 prerequisite.
- `npx @uipilot/plugin-cli validate` for directories and archives.
- `--platform` behavior and defaults.
- Human versus JSON output.
- Exit-code use in CI.
- The fact that validation does not execute or install the plugin.

Registry publication instructions are not added until the package is actually published.

## Acceptance Criteria

- A developer with Node.js 20 and the packed npm artifact can validate a plugin without Rust, UiPilot, or the UiPilot source tree.
- Directory and archive inputs enforce the same documented package constraints.
- Valid `demo-win` and `demo-return` packages pass for Windows.
- Invalid fixtures return stable issue codes and the specified exit status.
- macOS targeting rejects Windows-only permissions.
- JSON mode emits exactly one valid DTO and no progress output.
- Validation never executes plugin code or writes into the selected source.
- The packed-artifact smoke test passes from a fresh temporary project.
