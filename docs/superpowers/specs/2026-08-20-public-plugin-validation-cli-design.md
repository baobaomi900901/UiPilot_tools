# Public Plugin Validation CLI Design

**Status:** Approved - four rounds of independent review passed, ready for implementation planning

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

Node.js and npm/npx are the only user-installed prerequisites. On Windows, the directory validator may call the Windows PowerShell and .NET components supplied by supported Windows installations; the npm package never downloads or installs a native helper.

CLI release version and host compatibility are separate. This release validates against:

- UiPilot host version `0.2.0`.
- Public Plugin API version `1`.
- Package Schema version `1`.

The executable name is `uipilot-plugin`. The CLI uses a subcommand-oriented command parser so the later packaging task can add `pack` without changing the `validate` contract.

The npm artifact contains one bundled ESM executable with every runtime dependency, the UiPilot Plugin v1 JSON Schema, and generated Unicode 15.1 normalization and folding data. Runtime `dependencies` are empty after bundling. The package `files` allowlist contains only `dist/`, `README.md`, `LICENSE`, `THIRD_PARTY_NOTICES`, and `package.json`; the executable does not resolve code, Schema, Unicode data, or dependencies from a parent directory.

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

The directory reader uses no-follow metadata and rejects symbolic links, junctions, reparse points, sockets, devices, and any entry that is not an ordinary file or directory. On non-Windows systems, `lstat` is the no-follow type authority.

On Windows, Node `lstat` alone is not authoritative because it does not expose every `FILE_ATTRIBUTE_REPARSE_POINT` case. The pure TypeScript CLI therefore owns a narrow Windows OS adapter that invokes a verified absolute Windows PowerShell path with a fixed, bundled script.

Before parsing or opening the supplied source, the adapter captures `SystemRoot` and `WINDIR` from the inherited process environment. Both must be local drive-absolute paths and must resolve with `realpath.native` to the same directory under Windows ordinal case-insensitive comparison. That frozen directory is the system root for the entire validation run; later environment mutation cannot change it. The adapter constructs `<resolved-system-root>\System32\WindowsPowerShell\v1.0\powershell.exe`. The candidate must be an ordinary file, its native real path must equal the constructed path under the same comparison, and it must remain beneath the resolved system root. The adapter never passes a bare executable name and never searches the application directory, source directory, current directory, or `PATH`. It sets `cwd` to the verified `<resolved-system-root>\System32` directory and sets `TEMP`/`TMP` to a verified ordinary `<resolved-system-root>\Temp` directory. Its minimal child environment contains only the frozen `SystemRoot`, `WINDIR`, `TEMP`, and `TMP`; it does not pass `PATH`. The child starts with `windowsHide: true`, `-NoLogo -NoProfile -NonInteractive`, and a fixed UTF-16LE `-EncodedCommand` payload. Any path, directory, real-path, or executable verification failure is `SOURCE_INVALID`.

Each invocation accepts one JSON request of at most 256 KiB containing the absolute root once plus at most 321 canonical package-relative paths. Node writes UTF-8 without BOM. The fixed script explicitly sets Console input/output and PowerShell `$OutputEncoding` to strict UTF-8 without BOM, reads the complete request from Console stdin, combines each relative path beneath the root, calls .NET `File.GetAttributes`, and writes one compact JSON response. Node fatal-decodes at most 256 KiB of stdout. Paths are data only and are never interpolated into a command, script, environment value, or argument. The child has a five-second deadline; stderr, nonzero exit, timeout, missing PowerShell, malformed encoding/output, missing entries, or an attribute-query error all fail closed as `SOURCE_INVALID`.

The Windows reader checks the root and every discovered entry before access and after the corresponding read or directory enumeration. File reads use an open handle and compare no-follow path metadata with handle metadata before and after reading; directory enumeration rechecks directory metadata and attributes after enumeration. Any reparse attribute, identity/type change, size change, replacement, disappearance, or newly unqueryable path invalidates the entire snapshot. The package ships no native helper and requires no installed component beyond Node.js and the Windows system facilities already present on supported Windows versions.

The archive reader processes the central directory lazily with raw filename decoding disabled. It decodes every raw name with fatal UTF-8, rejects encrypted or unsupported compression entries, checks declared and actual uncompressed sizes, and never extracts the archive to disk. Only Stored and Deflate compression are accepted. Every file stream is consumed through EOF while computing CRC-32; its result must equal the unsigned central-directory CRC-32 exactly. A CRC mismatch, premature EOF, decompressor error, or bytes after the declared uncompressed size is `ARCHIVE_INVALID`.

For every central-directory entry, the reader inspects the creator system and external file attributes. When a Unix mode is present, `mode & 0o170000` must be zero or exactly `0o040000` for a directory entry and zero or exactly `0o100000` for a file entry. Symlink, socket, FIFO, block-device, character-device, and file/directory type mismatches are `ARCHIVE_INVALID`. Parent directories implied by file entries participate in the same directory count and case-fold collision rules as explicit directory entries.

Unsafe source acquisition is fail-fast. Once a safe bounded snapshot exists, independent validators may collect multiple issues.

### Package Policy

The package-policy module owns the host-equivalent constants and pure path/resource rules. It applies Unicode 15.1 `NFC -> full case fold -> NFC` when finding case-insensitive collisions.

Both normalization and folding are fixed to Unicode 15.1. The build generates bundled tables for canonical decomposition, canonical combining class, composition exclusions/composition pairs, and full default case-fold mappings from pinned Unicode 15.1 data. NFC additionally implements the UAX #15 algorithmic Hangul decomposition and composition using the fixed `SBase`, `LBase`, `VBase`, `TBase`, `LCount`, `VCount`, `TCount`, `NCount`, and `SCount` constants; table-only normalization is forbidden. The runtime uses the generated pure TypeScript NFC and fold implementation for all policy decisions and does not call `String.prototype.normalize`, `toLowerCase`, locale APIs, or the Node/ICU normalizer. Fixed normalization, folding, Hangul, and recomposition vectors identify the algorithm as `uipilot-unicode-15.1-full-fold-nfc-v1`.

The source tree and archive share the same canonical-path and resource checks. Path identity never relies on locale-sensitive lowercasing.

### Manifest Validation

Manifest validation starts from the raw `plugin.json` bytes. A fatal UTF-8 decoder rejects replacement-decoded input, then a duplicate-key-aware strict JSON parser rejects duplicate object members at every nesting depth and rejects nesting deeper than 128 containers, matching the bounded host parser. JSON string escapes must decode to Unicode scalar values: high surrogates must be followed immediately by a low surrogate, and isolated, reversed, or unpaired surrogates in keys or values are invalid. Invalid UTF-8, invalid JSON syntax, invalid surrogate escapes, duplicate keys, non-finite numeric results, and parser-limit failures map to `MANIFEST_JSON_INVALID`. Only the successfully parsed value is passed to the generated standalone Schema validator.

At build time, Ajv strict mode compiles the pinned JSON Schema into a standalone ESM validator. The build registers `uint32` and `double` with standalone code definitions so their checks are inlined into the generated module: `uint32` accepts only integers in `0..=4294967295`, and `double` accepts only finite JSON numbers. Unknown formats, Schema compilation failure, or standalone-code generation failure abort the build. The packed runtime does not include Ajv, does not compile a Schema, and does not use `eval` or the `Function` constructor. Failure to load or invoke the already-generated validator is the unexpected runtime error `CLI_INTERNAL`; ordinary validation failures remain `MANIFEST_SCHEMA_INVALID`. JSON parsing and Schema failures remain distinct. After Schema validation, semantic validation enforces host contracts that JSON Schema alone cannot express, including output-mode/window/permission relationships, canonical versions, target-platform compatibility, API compatibility, setting defaults, unique arrays, and platform-specific permissions.

### Specialized Validators

- The entry validator requires the declared Runtime and optional window entry to exist with the exact allowed extension.
- Before `pngjs`, the icon validator runs a bounded raw PNG chunk walker. It validates the signature, chunk boundaries, every chunk CRC including unknown ancillary chunks, required IHDR/IEND order, exactly one terminal IEND, no trailing bytes, and rejects `acTL`, `fcTL`, or `fdAT`. It then uses `PNG.sync.read({ checkCRC: true })` for complete pixel decoding, requires `128 x 128`, and applies the byte limit.
- The CSS validator first decodes each stylesheet with fatal UTF-8, then deliberately reproduces the current Rust ASCII-insensitive text scanner rather than using a standards-aware CSS parser. It scans the complete text, including comments and quoted strings, for exact `url(` and `@import` tokens and resolves accepted references only within the package snapshot. Scanner locations are reported as UTF-8 byte offsets. Its trim predicate is the fixed Rust-compatible White_Space set: `U+0009..U+000D`, `U+0020`, `U+0085`, `U+00A0`, `U+1680`, `U+2000..U+200A`, `U+2028`, `U+2029`, `U+202F`, `U+205F`, and `U+3000`.
- The report builder keeps a bounded ordered set containing at most the smallest 100 unique issues. Its total sort key is `(phaseRank, canonicalPathBytes, codeRank, ruleRank, locationKindRank, locationValueBytes)`. Phase ranks are package/source `10`, Manifest presence/JSON `20`, Manifest Schema `30`, Manifest semantics `40`, platform/API/permission `50`, declared entries `60`, icon `70`, and CSS `80`. Code ranks follow the declaration order in `PluginValidationIssueCode`; every concrete rule has a committed numeric `ruleRank`. A missing path sorts before every present path. A missing location has kind rank zero; `jsonPointer`, `byteOffset`, and `name` have ranks one, two, and three. JSON Pointer values use RFC 6901 escaping, byte offsets use canonical unsigned decimal without leading zeroes except `0`, and name values match `[a-z][a-z0-9.-]{0,63}`. Strings compare by UTF-8 bytes, and identical keys are deduplicated. Seeing any additional unique issue sets `truncated` to `true` without retaining unbounded detail.

No validator evaluates HTML, CSS, or JavaScript.

## Package Validation Rules

The CLI matches the UiPilot `0.2.0` host rules:

- At most 64 directories and 256 files.
- At most 8 path components.
- At most 2 MiB per file and 16 MiB total uncompressed file content.
- At most 240 UTF-8 bytes per path and 100 UTF-8 bytes per component.
- Archive file size at most 16 MiB and no more than 320 central-directory entries.
- A Unix ZIP entry type, when present, must be zero or match exactly an ordinary file or directory; links and special files are rejected.
- Paths must be relative NFC text using `/` separators.
- Empty components, `.`, `..`, trailing dot/space, control characters, Windows-forbidden characters, Windows reserved names, and full-fold collisions are rejected.
- `plugin.json` is required at the archive or directory root.
- Resource basenames have exactly one extension.
- Allowed extensions are `.html`, `.js`, `.css`, and `.png`; the only PNG resource is optional root `icon.png`.
- `icon.png` is at most 128 KiB, exactly `128 x 128`, fully decodable, valid through IEND with no trailing data, CRC-valid for every chunk, and non-animated.
- CSS files must be strict UTF-8. The compatibility scanner ASCII-folds only `A..Z`, recognizes exact `url(` with no whitespace, takes the first following `)`, trims the fixed Rust-compatible whitespace set, removes every leading or trailing `'` or `"` character exactly as Rust `trim_matches` does, and validates the resulting reference. It also recognizes exact `@import` and takes the first following `;`. When the trimmed import ASCII-folds to a value starting with exact `url(`, no media-remainder check is added beyond the separate URL scan; otherwise it requires one leading quoted reference whose closing matching quote is followed only by the fixed whitespace set. `url (` is not recognized; comments and strings are not skipped; nested parentheses end at the first `)`; a missing delimiter is invalid.
- CSS references are non-empty package-local paths without protocols, absolute paths, backslashes, query strings, fragments, percent encoding, or control characters.
- Referenced CSS resources must exist after canonical relative resolution.
- Manifest `supportedPlatforms` must contain the selected target.
- Manifest API version must equal `1` and `minimumHostVersion` must not exceed `0.2.0`.
- Only permissions implemented by the selected target are accepted. In the current host contract, `ui.window` and `clipboard.write` are available on both targets, while `notifications.publish` is available only on Windows; other declared permissions are rejected.

## Data Flow

1. Parse arguments and choose target platform.
2. Attempt to classify the source as directory or archive without following links; retain `unknown` if classification cannot be established safely.
3. Acquire a bounded, canonical package snapshot, including Windows pre/post attribute checks or ZIP external-attribute checks.
4. Fatal-decode and duplicate-aware parse `plugin.json`, then Schema-validate it.
5. Run Manifest semantic and compatibility validation.
6. Validate declared entries, resources, icon, and CSS references.
7. Sort and cap issues, render the selected output, and return the fixed exit code.

If safe snapshot acquisition fails, later steps do not run and the report contains one fatal source/archive issue with `truncated: false`. A validator may not turn a prior fatal source error into a partial success.

After a safe snapshot exists, validator dependencies are fixed:

- Missing `plugin.json` emits `MANIFEST_MISSING`; JSON, Schema, semantic, compatibility, and declared-entry validation are skipped, while independent icon and CSS validation still run.
- Invalid UTF-8, duplicate keys, or invalid JSON emits `MANIFEST_JSON_INVALID`; Schema, semantic, compatibility, declared-entry, and plugin-summary production are skipped, while icon and CSS validation still run.
- Schema failure emits bounded `MANIFEST_SCHEMA_INVALID` issues; semantic, compatibility, declared-entry, and plugin-summary production are skipped, while icon and CSS validation still run.
- After Schema success, semantic, platform, API, permission, and declared-entry validators all run in their fixed phase order even if an earlier one reports an issue. The optional plugin summary may then be emitted because its fields are structurally typed.
- Icon and CSS validation run after every safe snapshot regardless of Manifest validity.

## JSON Output

Validation results use this DTO:

```ts
interface PluginValidationReportV1 {
  schemaVersion: 1;
  valid: boolean;
  source: {
    kind: "directory" | "archive" | "unknown";
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
  truncated: boolean;
  issues: Array<{
    code: PluginValidationIssueCode;
    path?: string;
    location?:
      | { kind: "jsonPointer"; value: string }
      | { kind: "byteOffset"; value: string }
      | { kind: "name"; value: string };
    message: string;
  }>;
}
```

`source.path` preserves the source argument supplied by the caller. `source.kind` is `unknown` when the path is missing, has the wrong extension, is a root link/reparse point, is not an ordinary file/directory, or cannot be safely classified. Those inputs return `PluginValidationReportV1`, `SOURCE_INVALID`, `truncated: false`, and exit code `1`; they are not usage errors. Issue paths always use canonical package-relative `/` paths. `location.kind` distinguishes RFC 6901 JSON Pointers, canonical unsigned-decimal UTF-8 byte offsets, and fixed validator names; `location.value` follows the syntax frozen by the report-builder contract. The optional plugin summary appears only after Schema success; it is informational and does not imply validity.

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
- More than 100 unique reportable issues produces the first 100 by the fixed total order and sets `truncated: true`; 100 or fewer sets it to `false`.
- JSON mode remains valid JSON for all expected exit paths.
- Internal exceptions return the fixed `CLI_INTERNAL` response without a stack trace.

## Testing

### Unit Tests

- Argument parsing, platform defaulting, output rendering, and exit-code mapping.
- Canonical path validation, UTF-8 byte limits, Windows reserved names, and Unicode 15.1 decomposition, combining, recomposition, full-fold, and post-fold NFC vectors, including precomposed Hangul syllables, `L+V`, `LV+T`, and fold-then-Hangul recomposition.
- Windows OS-adapter protocol fixtures, including an ordinary-looking entry whose .NET attributes contain `ReparsePoint` while Node metadata reports a regular file; adapter unavailability and pre/post identity changes fail closed. A Windows integration probe exercises the verified absolute PowerShell path and fixed UTF-8 script without interpolating paths. Fixtures include a source-local fake `powershell.exe`, CJK/non-BMP names, a long absolute root sent once with relative paths, invalid UTF-8 output, and a missing/modified system executable. Privileged creation of a real non-link reparse point is optional, while the protocol fixture is release-gating.
- Manifest fatal UTF-8, duplicate-key, JSON syntax, isolated/reversed surrogate escapes in nested keys and values, generated standalone `uint32`/`double` format checks, Schema, and semantic validation.
- CSS strict UTF-8 and compatibility scanning fixtures covering comments, strings, `url(`, `url (`, quoted/unquoted values, media clauses, missing delimiters, and nested parentheses.
- Raw PNG fixtures covering unknown-chunk CRC, truncated chunks, missing/duplicate IEND, trailing bytes, APNG chunks, dimensions, and full `pngjs` decode.
- Total issue sorting, deduplication, dependency-based validator skipping, and bounded top-100 collection with `truncated` behavior.

### Integration Tests

- Valid directory packages for `mainResult` and `window` modes.
- Valid Stored and Deflate archives.
- Encrypted, malformed, traversal, duplicate, case-fold-collision, unsupported-resource, oversized, and missing-entry packages.
- ZIP fixtures whose Unix external attributes declare a symlink, socket, FIFO, block device, character device, or mismatched file/directory type.
- Stored and Deflate ZIP fixtures with a central-directory CRC-32 mismatch but unchanged uncompressed length.
- Platform, API, and permission incompatibility.
- Both human and JSON command output with exact exit codes.
- Repository `demo-win` and `demo-return` packages.

### Artifact Smoke Test

The build bundles the CLI, runtime libraries, generated standalone Schema validator, and generated Unicode tables into `dist/`, then creates an npm `.tgz`. A tarball-content assertion rejects any file outside the package allowlist and verifies that the bin target, package license, and third-party notices are present. The bundler metafile and final-bundle audit assert that neither Ajv nor any Schema compiler enters the runtime bundle. They allow only the fixed non-network Node built-ins required for local validation (`fs`, `path`, `os`, `crypto`, `zlib`, `stream`, `util`, `buffer`, `events`, `string_decoder`, `url`, `assert`, and Windows-only `child_process`) and reject `net`, `http`, `https`, `http2`, `tls`, `dgram`, `dns`, `undici`, dynamic external imports, `createRequire`, `process.binding`, `eval`, and `Function`. `child_process` may appear only at the single audited Windows adapter callsite whose executable and encoded script are fixed by this specification. The final bundle also rejects unbound uses of `fetch`, `WebSocket`, and `EventSource`. An artifact test imports the generated validator without Ajv present and proves one valid and one invalid Manifest result before the full smoke run.

The smoke harness copies its directory/archive fixtures and the `.tgz` into a new system temporary directory outside the repository, uses an empty npm cache, clears `NODE_PATH`, points registry/proxy settings at an unreachable endpoint, enables npm offline mode, and installs only the tarball with lifecycle scripts disabled. It first invokes `node_modules/.bin/uipilot-plugin --help` to verify the published bin mapping. It then resolves that mapping inside the installed package and launches the bundled ESM entry directly with `node --experimental-permission`, restricting filesystem reads to the temporary installation and fixtures; Windows additionally passes `--allow-child-process` only so the verified absolute PowerShell adapter can run. A preload trap replaces Node `net`, `http`, `https`, `http2`, `tls`, `dgram`, and `dns` connection APIs plus global `fetch`, `WebSocket`, and `EventSource` with deterministic failures before synchronizing built-in ESM exports. Both directory and archive validation must pass.

The Node 20 permission model is used only to prove filesystem/source isolation; the specification does not claim that it restricts network access. Offline npm installation, the build-time capability allowlist, and the runtime API trap together prove that the packed CLI has no supported Node network path. A stronger OS-level egress sandbox may be added in CI but is not an MVP acceptance dependency. Any parent `node_modules` lookup, repository Schema lookup, filesystem read outside the temporary root, undeclared runtime dependency, or trapped network call fails the smoke test. The harness never renames or changes permissions on the developer's repository.

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
- Missing, wrong-extension, root-link, and root-reparse sources return `kind: "unknown"`, `SOURCE_INVALID`, and exit code `1`.
- macOS targeting rejects Windows-only permissions.
- JSON mode emits exactly one valid DTO, includes correct `truncated` state, and emits no progress output.
- Validation never executes plugin code or writes into the selected source.
- Windows attribute probing fails closed and rejects a non-symlink/non-junction reparse protocol fixture.
- ZIP Unix external-attribute fixtures reject every link, device, and type mismatch.
- Stored and Deflate archive entries with incorrect CRC-32 are rejected.
- Manifest duplicate keys and invalid UTF-8 are rejected before the standalone Schema validator.
- Manifest strings with isolated or reversed surrogate escapes are rejected before the standalone Schema validator.
- PNG, Unicode, and CSS compatibility fixtures match the frozen host rules.
- The packed-artifact smoke test passes from a fresh system temporary project with offline installation, no repository or parent dependency access, a network-capability-free bundle graph, and active network API traps.
