# Stage 2D Task 3 — Strict Launch Bootstrap Metadata

**Date:** 2026-09-20
**Base:** `b25e209`.
**Status:** Increment implemented and independently reviewed; Task 3 remains IN PROGRESS.

`BootstrapArgs` now parses the helper's bootstrap string in a fixed five-field
order: package version, request ID, pipe nonce, host PID and host creation time.
It rejects reordered/duplicate/extra fields, non-lowercase or non-64-character
IDs/nonces, invalid versions and zero identities. `launch_runas` validates these
metadata before `ShellExecuteExW` and checks the bootstrap version against the
trusted package version.

Verification:

- Host launch-focused tests: 6 passed, 0 failed.
- Helper strict Clippy: passed.
- Formatting and `git diff --check`: passed.
- Independent `bootstrap_parser_review`: no correctness findings.

Tests cover rejection ordering and pre-Shell validation; no UAC or native
ShellExecute invocation was performed. Helper parsing/peer exchange and full
launch coordinator wiring remain outstanding.
