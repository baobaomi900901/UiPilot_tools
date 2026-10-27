# Stage 2D Task 3 — Native Peer Observation

**Date:** 2026-09-15
**Status:** Increment implemented and independently reviewed; Task 3 remains IN PROGRESS.

Shared source `src-tauri/src/process_elevation/peer.rs` is compiled by both the
host and the independent helper package. `ObservedPeer::from_pipe` obtains the
actual client/server PID from Windows, compares the expected PID and creation
time, and retains a non-inheritable QUERY_LIMITED_INFORMATION | SYNCHRONIZE
process handle. It observes TokenUser, TokenSessionId and the Win32 image path,
checks liveness before and after observation, and checks the pipe PID again.
The caller can later recheck liveness on the retained handle.

TokenUser buffers are aligned and bounded to 64 KiB; the SID header and
subauthority bytes must lie inside the returned buffer before SID conversion.
Image paths preserve UTF-16 via OsString rather than lossy text conversion.
Process/token handles and converted SID allocations are RAII-owned. Errors expose
stable classifications; no peer facts implement Debug or enter normal logs.

This observer intentionally does not mint authenticated sessions. An observed
image path does not prove its loaded image digest, protected installation,
version, administrator status or integrity level. Those checks and full pipe
handshake integration remain required before any production execution path.
The helper executable still exits closed; no new UI, UAC or stop operation is wired.

## Validation

Initial RED on the empty observer: both new native tests failed with
PeerUnavailable instead of producing valid observations/identity rejections.
After implementation, both endpoints of an owned pipe were observed successfully.
Zero/mismatched expected PID/time and an invalid pipe handle reject with their
specific stable classifications.

An additional owned-child case establishes a pipe connection, observes that exact
child, releases its stdin gate, waits for normal exit, and verifies that the
retained process handle now reports PeerNotAlive. Cleanup is bounded and only
targets that explicitly spawned child; it never searches for arbitrary processes.

- Helper focused peer suite: 3 passed, 0 failed, 1 ignored child entrypoint.
- Independent helper full suite: 13 passed, 0 failed, 1 ignored child entrypoint.
- Host `process_elevation` suite: 22 passed, 0 failed, 1 ignored child entrypoint.
- Helper all-target strict Clippy: passed.

Commands used from the worktree root:

```text
cargo test --manifest-path src-tauri/process-elevation-helper/Cargo.toml --locked --offline -- --test-threads=1
cargo clippy --manifest-path src-tauri/process-elevation-helper/Cargo.toml --locked --offline --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib process_elevation:: -- --test-threads=1
```

The host command used the existing isolated `.task3-target` output directory.
No full application regression or inherited Clippy debt rerun was needed for
this private observer increment; no claim clears those earlier red baselines.

Independent `native_peer_observation_review` reviewed buffer bounds, identity
ordering, RAII and scope, independently ran the helper suite, and found no
blocking issue. Actual UAC/package-trust acceptance and prior manual gaps are
unchanged; this is not Task 3 or Stage 2D final acceptance.

## Image Digest Preparation Note

The next trust increment adds a SHA-256 helper that streams an absolute path and
rejects parent-directory components, malformed digests and mismatched bytes. Its
focused tests pass. It is deliberately not re-exported or called by the launch
wrapper yet: pathname hashing alone does not prevent reparse/junction resolution
or a concurrent replacement between hashing and `ShellExecuteExW`. The eventual
launch gate must hold a no-reparse file handle and bind the verified file identity
to the launched helper. This limitation is recorded as an open Task 3 integration
condition; the digest helper is not evidence of completed package trust.

Update: the path-only helper has now been replaced by a held image/ancestor
lease; see [2026-09-19 image lease evidence](./2026-09-19-process-elevation-image-lease.md).
Package ACL/manifest trust and actual GUID-path UAC compatibility remain separate
outstanding requirements.
