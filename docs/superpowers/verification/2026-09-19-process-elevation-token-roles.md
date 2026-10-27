# Stage 2D Task 3 — Native Account And Privilege Roles

**Date:** 2026-09-19
**Status:** Increment implemented and independently reviewed; Task 3 remains IN PROGRESS.
**Base:** `c452650`.

The shared `token.rs` module observes primary-token account SID, session,
TokenElevation, integrity SID and enabled administrator membership. Token buffers
are aligned and bounded to 64 KiB. Returned group arrays and embedded SIDs are
bounds-checked before access; converted SID and WTS buffers have scoped cleanup.
No token facts implement Debug or enter ordinary logs.

Role policy requires a medium-integrity, non-elevated host without enabled
administrator membership and a high-integrity, elevated helper with enabled
Administrators membership. Deny-only/disabled administrator groups do not qualify.
Account SIDs and nonzero sessions must match. SYSTEM, LocalService and NetworkService
identities reject. Both verification directions use the same host/helper policy.

`ObservedPeer::check_account_and_role` checks the retained peer handle's liveness,
reads local and peer token facts afresh, detects changes to the previously observed
peer account/session, applies the role policy and checks liveness again. Session
validation requires the active local-console session and WTS protocol type zero;
remote protocols, mismatched/no console and unavailable observations fail closed.
The old duplicated TokenUser reader in `peer.rs` is replaced by this shared reader.

These checks do not create an authenticated Session, prove package metadata or
version, or authorize a target operation. The helper entry remains fail-closed.
This is not a claim that Task 3 authentication integration is complete.

## Evidence

Initial RED: all three new role tests failed against an empty policy. Coverage now
includes correct opposite-role combinations in both directions; wrong accounts,
zero/mismatched sessions; service identities; incorrect elevation, integrity and
group state; remote/no-console classification; and SID pointer/range rejection.

Native checks read the current primary token and actual local WTS session without
elevation. Actual owned-pipe peer tests show that one process cannot satisfy both
the host and helper roles. Synthetic high-token fixtures prove policy decisions,
not an actual elevated-token success path.

- Helper full tests: 35 passed, 0 failed, 1 ignored owned-child entrypoint.
- Host process_elevation tests: 41 passed, 0 failed, 1 ignored owned-child entrypoint.
- Helper all-target strict Clippy and host/helper formatting: passed.

```text
cargo test --manifest-path src-tauri/process-elevation-helper/Cargo.toml --locked --offline -- --test-threads=1
cargo clippy --manifest-path src-tauri/process-elevation-helper/Cargo.toml --locked --offline --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib process_elevation:: -- --test-threads=1
```

Host tests used `.task3-target`. Independent `native_token_role_review` ran the
helper suite and reviewed native buffers, role direction and cleanup. It identified
a 64-bit assumption in a synthetic SID test buffer; allocation now covers the
declared size on both pointer widths. The targeted SID test and independent source
recheck passed. No remaining review finding. No 32-bit build is claimed.

No UAC, installation, ACL change, GUI or external process operation was performed.
Actual cross-account/RDP rejection and real elevated success with this production
module remain untested; earlier spike results are not substituted for those checks.
