# Stage 2D Task 3 — Strict Package Manifest Parsing

**Date:** 2026-09-20
**Base:** `8ceb80c`.
**Status:** Preparatory increment verified; Task 3 remains IN PROGRESS.

`TrustPackage::from_manifest` now parses a bounded 64 KiB JSON manifest with a
custom strict visitor. Unknown fields, missing fields and duplicate fields are
rejected before package construction. Version text is retained on
`TrustPackage` and limited to a nonempty 64-byte build-version token; host and
helper digests are validated as lowercase SHA-256-shaped values.

The first independent review found duplicate-field acceptance and discarded
version metadata. Both were fixed; focused tests cover duplicate fields and the
retained version accessor. A follow-up review then found that manifest
construction bypassed the package-root invariant; `new_with_version` now shares
the absolute/no-parent root check and the regression test covers it. The manifest parser is currently preparatory: no
production launch path reads it from disk yet, and it does not by itself prove
protected installation provenance or host/helper version equality.

Verification:

- Host `process_elevation` tests: 44 passed, 0 failed, 1 ignored.
- Helper full tests: 38 passed, 0 failed, 1 ignored.
- Helper trust-focused tests: 4 passed, 0 failed.
- Helper strict Clippy and formatting: passed.
- `git diff --check`: passed.

No UAC, installation, ACL modification or process operation was performed.
