# Stage 2D Task 3 — Held Image Digest And Launch Binding

**Date:** 2026-09-19
**Status:** Increment implemented and independently reviewed; Task 3 remains IN PROGRESS.
**Base:** `b52ef6d`.

The previous path-only digest function closed its file before launch. It has been
removed. Shared `process_elevation/image.rs` now returns an owned `ImageLease`:

- Accept local absolute DOS paths; reject network/device paths, parent/dot
  components, alternate streams, trailing-dot/space aliases and unsupported text.
- Open the volume root without following reparse points, verify fixed NTFS, and
  derive its volume-GUID path. All subsequent component opens and the Shell launch
  path use that GUID rather than re-resolving the DOS drive alias.
- Retain each directory and the image with share-read only, denying concurrent
  write/delete access. Reject every reparse point, unexpected object kind or final
  path mismatch. Record volume serial plus file index from the held image handle.
- Compute SHA-256 from that same held file. Reject images above 512 MiB and failed
  or inconsistent reads. Any error releases all acquired handles through RAII.
- `TrustPackage::lease_helper_image` selects the fixed helper filename and expected
  digest. `launch_runas` keeps the lease across ShellExecuteExW; `ElevatedProcess`
  retains it until the process-handle owner is dropped.
- On a returned process handle, compare the observed process image path's held
  volume/file identity and final GUID path with the pre-launch lease, checking
  process liveness before and after. A mismatch closes the returned handle and
  releases the lease; it does not kill a possibly started process.

This binds held on-disk image identity to the launch path/returned-process path.
It does not independently attest mapped process memory, validate package ACLs,
publisher identity or immutable manifest provenance. `TrustPackage` alone is still
not a complete authenticated-installation proof. No production command entry or
helper authentication gate has been enabled by this change.

## Evidence

Initial RED: two tests failed against the empty lease implementation (matching
image acquisition and digest-mismatch classification). After implementation:

- Matching `abc` fixture digest succeeds; writes, image rename and ancestor rename
  fail while held; writes succeed after drop.
- Digest mismatch releases file/directory handles, allowing the owned directory
  to be renamed afterwards.
- A real test-owned NTFS junction is rejected in the ancestor chain.
- Network/device namespaces, parent/dot aliases and alternate streams reject before
  file access. No external network path is opened.
- The current test process matches a lease on its own executable. A different
  owned image is rejected as PeerImageMismatch.

Commands (host uses the existing isolated `.task3-target`):

```text
cargo test --manifest-path src-tauri/process-elevation-helper/Cargo.toml --locked --offline image::tests -- --test-threads=1
cargo test --manifest-path src-tauri/process-elevation-helper/Cargo.toml --locked --offline -- --test-threads=1
cargo clippy --manifest-path src-tauri/process-elevation-helper/Cargo.toml --locked --offline --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib process_elevation:: -- --test-threads=1
```

Results: image tests 5 passed; helper full tests 18 passed / 0 failed / 1 ignored
owned-child entrypoint; host focused tests 27 passed / 0 failed / 1 ignored.
Helper strict Clippy and host/helper formatting checks passed. The helper adds
the host's pinned sha2 0.10.9 dependency; no Tauri dependency is introduced.

Independent `image_lease_review` ran the helper suite and reviewed path handling,
sharing, file identity and RAII. No concrete blocker remained. Actual volume-GUID
ShellExecute/UAC compatibility and returned elevated-process verification were
NOT exercised; the current-process test does not substitute for that integration.
No installation, UAC, GUI or target-process operation was performed.
