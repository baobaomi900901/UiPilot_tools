# Stage 2D Task 3 — Native Pipe Client

**Date:** 2026-09-20
**Base:** `f4a0a01`.
**Status:** Increment verified and independently reviewed; Task 3 remains IN PROGRESS.

The shared `PipeStream::open_client` opens only a validated `LocalPipeName`,
using `OPEN_EXISTING`, no inheritable security attributes and
`SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION`. Specific read/write,
attribute and synchronization rights exclude `FILE_CREATE_PIPE_INSTANCE`.
Missing or busy endpoints fail immediately without a wait/retry loop.
The returned stream owns the handle and closes it on drop.

Existing native peer tests, including the owned child exit fixture, now open
their client through this production function rather than a duplicated test
`CreateFileW` call. The fixture receives only a validated nonce.

Initial RED: both new client tests failed on connection with the unimplemented
opener. After implementation, real local-pipe tests check duplex bytes,
non-inheritable handles on both ends, missing/busy endpoint rejection and
closure after drop. After reading client bytes, the test calls
`ImpersonateNamedPipeClient` and observes the actual thread token's
`TokenImpersonationLevel == SecurityIdentification`. It reverts on every
scope exit (aborting the test process if reversion itself fails).

This token observation uses the current ordinary test process, not an elevated
helper. It establishes the production opener's SQOS behavior, not real UAC
compatibility or complete mutual authentication. Tests use an owner-only fixture
DACL; production initiating-logon DACL construction remains separate work.

Verification so far:

- Helper full tests: 37 passed, 0 failed, 1 ignored owned-child entrypoint.
- Host `process_elevation` tests: 43 passed, 0 failed, 1 ignored owned-child
  entrypoint (28.70 s build, 1.92 s tests).
- Helper all-target strict Clippy: passed.
- Host and helper formatting and `git diff --check`: passed.

Independent `pipe_client_review` reviewed the diff against `f4a0a01` for
correctness, security and scope and independently ran the helper suite
(37 passed, 0 failed, 1 ignored). No findings remain.

Commands:

```text
cargo test --manifest-path src-tauri/process-elevation-helper/Cargo.toml --locked --offline -- --test-threads=1
cargo clippy --manifest-path src-tauri/process-elevation-helper/Cargo.toml --locked --offline --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib process_elevation:: -- --test-threads=1
```

Host tests use the isolated `.task3-target` directory. No UAC, installation,
ACL modification or arbitrary process operation is performed. The client returns
an unauthenticated transport; helper main stays fail-closed. Package metadata,
mutual-authentication integration and cancellable/deadline-bound I/O remain
outstanding. Task 3 is not complete; Tasks 4–6 and roadmap Stage 3 are not started.
