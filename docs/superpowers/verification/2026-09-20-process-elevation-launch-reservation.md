# Stage 2D Task 3 — Launch Reservation Cleanup

**Date:** 2026-09-20
**Base:** `5d1a8ec`.
**Status:** Increment implemented and independently reviewed; Task 3 remains IN PROGRESS.

`LaunchReservation` now releases the coordinator's single pending slot whenever
the reservation is dropped. The previous `returned` flag left the slot occupied
forever when an owner abandoned a reservation before `complete`; this could
permanently block later helper launches. The synchronous production launcher
holds the reservation through ShellExecute and process identity observation, and
`complete` still consumes it before RAII cleanup.

The regression test was changed first and failed against the old behavior, then
the drop implementation was corrected. The independent
`launch_reservation_review` checked all callers and found no ownership race or
stranded-slot regression.

Verification:

- Host launch-focused tests: 5 passed, 0 failed.
- Helper strict Clippy: passed.
- Formatting and `git diff --check`: passed.

This fixes reservation cleanup only; native launch remains synchronous and full
deadline/cancellable transport is still outstanding. No UAC, installation, ACL
modification or process operation was performed.
