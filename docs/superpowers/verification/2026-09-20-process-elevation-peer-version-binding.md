# Stage 2D Task 3 — Peer Identity Version Binding

**Date:** 2026-09-20
**Base:** `df077b3`.
**Status:** Contract increment implemented and independently reviewed; Task 3 remains IN PROGRESS.

`PeerIdentity` now carries `package_version`. Because peer verification requires
exact identity equality and separately validates the version token, a version
mismatch or malformed version is rejected with `PeerRejected` alongside PID,
creation-time, SID, session and image-digest mismatches.

Verification:

- Host trust-focused tests: 4 passed, 0 failed.
- Helper trust-focused tests: 4 passed, 0 failed.
- Helper strict Clippy: passed.
- Formatting and `git diff --check`: passed.
- Independent `peer_version_review`: no correctness findings.

This extends the private identity contract only. `ObservedPeer` does not yet
populate package versions, and launch/session exchange is still unwired; no
end-to-end version enforcement is claimed. No UAC, installation, ACL change or
process operation was performed.
