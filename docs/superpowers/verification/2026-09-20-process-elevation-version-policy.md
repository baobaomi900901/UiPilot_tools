# Stage 2D Task 3 — Package Version Equality Policy

**Date:** 2026-09-20
**Base:** `f2abea8`.
**Status:** Policy increment implemented and independently reviewed; Task 3 remains IN PROGRESS.

`TrustPackage::verify_version` accepts only a valid version token that exactly
matches the retained manifest version. Mismatches and malformed values return
`PeerRejected`. This provides the comparison rule needed when host/helper
metadata is later loaded and exchanged through the authenticated coordinator.

Verification:

- Host trust-focused tests: 4 passed, 0 failed.
- Helper trust-focused tests: 4 passed, 0 failed.
- Helper strict Clippy: passed.
- Formatting and `git diff --check`: passed.
- Independent `version_policy_review`: no implementation findings.

The method is intentionally preparatory and currently has no production caller;
manifest loading, version exchange and launch/session wiring remain outstanding.
No UAC, installation, ACL modification or process operation was performed.
