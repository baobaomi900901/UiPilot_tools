# Stage 2D Task 1: Deployment And Authentication Feasibility

**Date:** 2026-09-09
**Status:** BLOCKED AT PACKAGING SCOPE GATE — inventory only; no native feasibility PASS.
**Inspected revision:** `2a57db3a546113a7d36a9011ad833a9f0a96958a`.
**Plan:** [Stage 2D Task 1](../plans/2026-09-09-windows-process-elevation.md).

## Observed Evidence

- `src-tauri/tauri.conf.json` enables bundle targets `all`, but has no
  `bundle.windows.nsis.installMode`, custom installer template/hooks or external
  helper binary entry. Resources list Everything assets and notification audio;
  no elevation helper or immutable host/helper digest manifest is present.
- `scripts/tauri.ps1` forwards build arguments to the local Tauri CLI. Its automatic
  override applies only to development server ports. The tracked
  `tauri.security-probe.conf.json` changes only the frontend build command.
  No inspected tracked release configuration establishes a protected elevation package.
- Cargo.lock pins tauri-utils 2.9.3. Its locally cached `src/config.rs` lines 818–840
  define NSISInstallerMode's default as CurrentUser (non-administrator installation)
  and PerMachine as the Program Files/admin installation alternative. NsisConfig
  uses that enum's default at lines 873–875. This establishes an NSIS default gap,
  not an assertion that every possible MSI or external installer is unsafe.
- The worktree lacks `node_modules/@tauri-apps/cli/config.schema.json` and the local
  CLI executable. No dependency installation was attempted. Rust dependencies are
  available; the preceding independent baseline at this same revision ran
  `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control:: -- --test-threads=1`:
  182 passed, 0 failed, 2 fixture entrypoints ignored. That is a process-control
  baseline only, not deployment or elevation evidence.

These observations do not prove effective ACLs of any installed application.
No installer was built/run, no installed files were probed or changed, and no
UAC/token/pipe/native-confirmation experiment was run. No Task 1 spike or runner
has been created: the packaging prerequisite needs an explicit scope decision first.

## Required Scope Decision

Design §3 and Task 1 explicitly require stopping if production packaging needs
broader changes. Current tracked configuration does not provide the required
trusted package. A manually protected test directory would demonstrate only the
fixture, so it cannot close this gate or authorize Task 2.

Recommended minimum expansion: add an opt-in, NSIS-only protected installation
profile for elevation-capable builds, retaining ordinary installation behavior.
This is a proposal, not an applied configuration or a claim that PerMachine alone
proves trust. The existing default/per-user builds must fail closed for elevation.

Proposed configuration file: `src-tauri/tauri.elevation.conf.json` with this initial
installation policy (helper/resources are added only when the approved artifacts exist):

```json
{
  "bundle": {
    "targets": ["nsis"],
    "windows": {
      "nsis": { "installMode": "perMachine" }
    }
  }
}
```

The requested expansion includes a build/package inventory for the host, fixed
helper, immutable digest/version manifest and every shipped dependency/resource;
an installer guard against user-writable or reparse destinations; and validation
that install/upgrade/uninstall preserve the protected package boundary. Use a
separate protected package identity/destination for experiments, never replace the
user's current installed host or change its permissions to create evidence.

Candidate files for that separately approved work are the opt-in config above,
`scripts/build-process-elevation-package.ps1`, and an NSIS hook file
`src-tauri/windows/elevation-installer-hooks.nsh` if needed for destination checks.
No auto-update framework, service, plugin signing, Stage 3 or default-installer
migration is included. If implementation needs those, stop and report separately.

## Next Checkpoint After Scope Approval

Prepare the approved Task 1 non-destructive probe and runner, build them locally,
verify their no-stop boundary and have an independent agent review them. Produce
the exact binary hashes, proposed absolute test destination, complete copy/ACL
inventory and bounded cleanup commands before requesting installation/UAC approval.
At that point authorization concerns concrete artifacts, not a generic permission
to run arbitrary elevated commands. No installation approval is requested here.

Until the native experiment actually runs, loader trust, same-account pipe/token
compatibility, peer binding, SQOS, UAC cancellation and confirmation UI remain
NOT RUN. Task 1 is not complete and Tasks 2–6 remain unstarted.

## Independent Review

`elevation_packaging_gate_review` on 2026-09-09: no blocking findings in this
inventory/gate report. Confirmed tracked configuration and pinned default;
older Everything design proposals are not implemented installer evidence.
Confirmed the approved design/plan requires separate packaging-scope approval.
Read-only review; no native tests, installation or UAC performed.
