# Stage 2D Task 3 — Native Authentication Proof Gate

**Date:** 2026-09-20
**Base:** `2537708`.
**Status:** Increment implemented and independently reviewed; Task 3 remains IN PROGRESS.

On Windows, `BoundSession::authenticate` now consumes an opaque
`peer::AuthenticationProof`. The proof type and field are private to the native
peer module. The only production constructor is inside
`ObservedPeer::authenticate_as`, after the retained peer has passed liveness,
account/session, role and local-console checks. No wire request, PID or SID can
construct this proof. The proof is consumed by authentication and cannot be
reused to authenticate another session.

The non-Windows facade retains its no-argument state-machine method only under
`cfg(not(windows))`; it is a portable protocol facade and is not a Windows
privileged path. The helper and host Windows builds use the proof-gated method.

An initial independent review found that the first implementation exposed a
parent-visible proof constructor and had no peer-to-session binding. That was
removed before commit. The targeted re-review confirmed no residual constructor,
no old Windows no-argument path and no findings.

Verification:

- Host `process_elevation` tests: 43 passed, 0 failed, 1 ignored.
- Helper full tests: 37 passed, 0 failed, 1 ignored.
- Helper focused session tests: 5 passed, 0 failed.
- Helper all-target strict Clippy: passed.
- Formatting and `git diff --check`: passed.

This gate still does not wire a production coordinator through launch, package
version/manifest provenance or deadline/cancellable transport. The helper entry
remains fail-closed; no UAC, installation, ACL modification or process operation
was performed. Task 3 is not complete.
