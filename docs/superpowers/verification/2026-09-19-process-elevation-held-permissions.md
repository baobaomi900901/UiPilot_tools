# Stage 2D Task 3 — Permissions On Held Installation Objects

**Date:** 2026-09-19
**Status:** Increment implemented and independently reviewed; Task 3 remains IN PROGRESS.
**Base:** `9e95eb5`.

Shared `process_elevation/permissions.rs` inspects Windows security descriptors
using the retained File handles from ImageLease. Directory opens now explicitly
request READ_CONTROL alongside FILE_READ_ATTRIBUTES. No pathname reopen is used
to make the permission decision, and no descriptor/ACL is modified.

Policy:

- Owners and write-capable trustees are restricted to SYSTEM, Administrators and
  TrustedInstaller. An ordinary-user owner rejects even if its ACL appears read-only.
- Missing/null or invalid DACLs and unsupported ACE types/flags reject.
- Ordinary allow ACEs may grant only read/execute, READ_CONTROL and SYNCHRONIZE,
  including their generic read/execute forms. Unknown permission bits reject.
- Ancestors may permit creating siblings; DELETE_CHILD, deletion, attribute writes
  and ACL/owner changes remain forbidden to untrusted trustees. The actual helper
  directory and image do not allow that sibling-creation exception.
- Deny rules do not override unsafe allow grants in this conservative policy.
  Inherit-only rules do not grant access to the current object; each actual held
  descendant is checked independently.

`TrustPackage::lease_helper_image` checks all held ancestors, the helper directory
and image before returning the lease to the launcher. A permission rejection drops
the lease. Existing image/directory non-reparse and share-read protections remain.
The same native permission code and tests compile in the independent helper crate;
there is still no Tauri dependency in that crate.

This is not a general Windows effective-access calculator and can reject complex
but otherwise safe ACLs. It is not proof of manifest/publisher authority, version
agreement, dependencies or authenticated peer roles. It does not protect against
a malicious administrator changing security state. Full Task 3 authentication and
actual GUID-path ShellExecute/UAC integration remain outstanding.

## Validation

Initial RED: 1 pass / 3 failures against the empty permission policy, demonstrating
missing owner, DACL and mutation-grant rejection. Native Windows SDDL conversion
provides the descriptors used by policy tests. The object-ACE fixture was corrected
to include an object GUID; an OA entry without a GUID did not exercise that case.

Coverage includes trusted owner/read-execute grants, untrusted owner, null/absent
DACL, every unsafe mutation bit, ancestor DELETE_CHILD, unsafe allow following
deny, unsupported object ACE, and unknown rights. A real Program Files directory
is inspected read-only through its handle. An owned temporary package with a
matching `abc` digest is rejected by the composed package-permission check;
subsequent writing proves the failed lease was released. This test never calls
the launcher, even if the permission assertion were to fail.

Results:

- Helper full suite: 28 passed, 0 failed, 1 ignored owned-child entrypoint.
- Host process_elevation suite: 34 passed, 0 failed, 1 ignored owned-child entrypoint.
- Helper strict all-target Clippy: passed.
- Host/helper formatting: passed.

```text
cargo test --manifest-path src-tauri/process-elevation-helper/Cargo.toml --locked --offline -- --test-threads=1
cargo clippy --manifest-path src-tauri/process-elevation-helper/Cargo.toml --locked --offline --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib process_elevation:: -- --test-threads=1
```

Host tests used the existing isolated `.task3-target`. Independent
`held_permissions_review` reviewed policy, native buffer/SID handling, RAII and
held-handle integration, independently ran the helper suite and found no
actionable issue. No UAC, installation, ACL change, GUI or arbitrary target-process
operation was performed. Inherited repository-level lint failures remain unchanged.
