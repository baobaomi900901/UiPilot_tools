# Everything CLI Tracer Bullet Design

**Status:** Approved for implementation on 2026-07-29.

## Goal

Provide the shortest executable proof that UiPilot can query a running Everything 1.4 instance through the same Query2 IPC code that will later power `/find`.

The tracer bullet extends the existing Rust binary in `spikes/everything-ipc`; it does not introduce a PowerShell wrapper, call `ES.exe`, or create a second query implementation.

## Preconditions

- Windows x64.
- Everything 1.4 is already installed, running, and has loaded its database.
- The default Everything instance is used unless `--instance` is supplied.

Installing, bundling, elevating, repairing, configuring, or changing Everything permissions is outside this slice.

## CLI Contract

```text
everything-ipc-spike \
  --query <everything-search> \
  [--instance <name>] \
  [--limit <1..200>] \
  [--timeout-ms <1..60000>] \
  [--format text|json]
```

Defaults:

- `instance`: default Everything instance
- `limit`: `20`
- `timeout-ms`: `1000`
- `format`: `text`
- sort: date modified descending

Text output prints one result per line. JSON output is one object containing:

- `total`
- `returned`
- `requestFlags`
- `sortType`
- `items[]`

Each item contains `fullPath`, `fileName`, `kind`, `sizeBytes`, `modifiedFiletime`, and `attributes`. `kind` is derived from the directory attribute and is either `file` or `directory`.

The CLI must not open results, mutate files, start Everything, or write configuration.

## Data Flow

1. Parse and validate CLI arguments.
2. Connect with the existing `EverythingClient`.
3. Submit one bounded Query2 request using the existing protocol encoder and reply window.
4. Validate the returned request flags and actual sort type.
5. Render the decoded `EverythingResultItem` values as text or JSON.
6. Exit without leaving a worker thread or hidden window behind.

No pagination is required because this tracer bullet requests at most 200 results in one Query2 request.

## Errors

Errors are written to stderr without file paths from partial or invalid replies.

- Invalid CLI arguments return a non-zero exit code.
- Missing Everything window returns a stable unavailable error.
- Query deadline expiration returns a stable timeout error.
- Invalid Query2/LIST2 data returns a stable protocol error.

The process must not panic for expected user or runtime errors.

## Tests

- Argument parsing: defaults, explicit values, duplicate options, invalid limit, invalid timeout, and unknown options.
- Rendering: file, directory, missing optional metadata, text escaping, and valid JSON.
- Existing protocol and fake-window tests remain green.
- One manual Windows smoke query against a running default Everything instance confirms real item paths, flags, sort type, and bounded exit.

## Production Alignment

The tracer bullet is accepted only if it directly uses `EverythingClient`, `EverythingQuerySpec`, and `EverythingQueryResult`. The next production slice will reuse those modules and replace CLI rendering with mapping into UiPilot's existing `FileSearchResponse`.

## Out Of Scope

- UiPilot installer changes
- Bundled Everything binaries
- Service lifecycle or UAC
- Owner, ACL, and multi-user policy
- Automatic refresh polling
- Removal of the existing Rust index
- Frontend changes

## Completion Criteria

- A user can run one command and see matching file and directory paths from Everything.
- Text and JSON modes return at most the requested limit.
- A missing or stopped Everything instance fails cleanly within the configured deadline.
- Existing spike tests pass.
- A real Everything smoke query succeeds on Windows.
