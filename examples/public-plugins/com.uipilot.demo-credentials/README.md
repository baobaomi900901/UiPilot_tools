# Credential Reference Demo

Windows Host `0.4.0+`, API v1, `submit + mainResult`. The package declares `service-token / api-token / api.example.com / bearer` and sends only `credential: 'service-token'`. It contains no token, dependencies, browser networking, panel, or window. `api.example.com` is a documentation placeholder, not a working provider. Never use a production token with this example.

`status` queries only `isSecretConfigured('api-token')`; the empty input or `request` demonstrates one Host request. Success displays bounded plain-text provider output, including non-2xx statuses. Errors display only a stable allowlisted name. A service may echo the token in normal output: the offline fake-provider test deliberately demonstrates that trust boundary rather than claiming redaction of provider responses.

Run from the repository root:

```powershell
node --test examples/public-plugins/com.uipilot.demo-credentials/tests/runtime.test.js
node node_modules/typescript/bin/tsc --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.demo-credentials/tests/sdk-contract.ts
npm run build --workspace @uipilot/plugin-cli
node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.demo-credentials/package --platform windows
node packages/plugin-cli/dist/cli.mjs pack examples/public-plugins/com.uipilot.demo-credentials/package --output ./demo-credentials.uipilot-plugin --platform windows
node packages/plugin-cli/dist/cli.mjs validate ./demo-credentials.uipilot-plugin --platform windows
```

The output must be outside the package and must not already exist. The isolated npm-consumer smoke also copies, validates, packs, and revalidates this example using the packed CLI artifact and offline network trap. Rust acceptance stages the actual development directory and compares resources/metadata without invoking or requiring a prebuilt CLI. Separate CLI/Rust tests explicitly reject Host `0.3.4` and accept `0.4.0`; these are automated static/staging checks, not an installed Host session.

The Node tests supply an offline fake `api.network.request`; the only token is a synthetic echo fixture in tests. They prove the plugin's reference and response/error handling, not Host DPAPI, TLS, cancellation, or grant enforcement. Host Rust tests cover those separate boundaries. No automated test here contacts the placeholder or any real service.

## Manual Windows Host acceptance (not performed by this task)

Use a disposable Windows Host `0.4.0+` profile with synthetic values only. These steps are for a human tester; this change does not install a plugin or alter system services.

1. Choose `package/` (or the locally packed archive) in plugin settings. Verify `service-token`, `api-token`, `api.example.com`, Bearer, and the service-echo warning are visible before consent. Cancel once and confirm no installation; then install and grant the displayed network declaration. A `0.3.4` Host must reject it.
2. Run `/demo-credentials status`: expect “Credential not configured”. With no stored value, `request` must return `CredentialUnavailableError` without a provider call. Do not run configured `request` against the placeholder domain.
3. In details, save the synthetic value `synthetic-demo-token` through the password control. Verify the input clears and `status` reports configured without showing any prefix/suffix. Clear explicitly, confirm status is unconfigured, and repeat with keyboard only (Tab/Shift+Tab, Enter, Escape) in Light and Dark.
4. Revoke network and verify requests fail with `PermissionDeniedError`. Re-enable only after confirming current complete bindings. In another settings view mutate inventory first: stale confirmation must fail, reload, and require fresh consent. Upgrade a disposable copy with a changed ID/key/host: confirm new consent; cancellation must retain the installed version. Reordering alone must not ask for new consent.
5. In a dedicated Host test harness with the existing fake transport, verify synthetic header injection, service echo, rotation/deletion/revocation while a request is held, and the running-slot/next-command behavior. Do not substitute a real service for this test. This repository's offline Runtime tests do not provide that installed-Host harness.
6. If a controlled persistence fault is available in the Host test environment, verify the UI refreshes configuration and explains partial commit: failure may leave old/new data, cancels old contexts after the cancellation point, and closes network for that call. Later startup/enable/grant/recovery checks actual disk and may reopen for a valid old/new or missing record. It must never revive old request IDs. A failed clear is not confirmed revocation; require successful clear or network-disable. Do not damage a real profile to simulate faults.

Cancellation cannot recall bytes already received by the service or completed results; it offers no remote rollback. Runtime and content have no secret getter; saved presence does not prove bearer validity. Bearer values are 1–8192 ASCII bytes matching `^[A-Za-z0-9._~+/-]+=*$`, with no `Bearer ` prefix or trimming. A request cannot combine `credential` and any `Authorization` header. See the [SDK contract](../../../docs/plugin-sdk/public-plugin-v1.md#host-managed-https) for fixed errors, limits, grant equality, partial-save recovery, and service response boundaries.

Actual Host installation/upgrade, native focus/keyboard, both themes, controlled persistence-fault UI, and installed fake-transport interaction remain manual/unrun until a tester records evidence. Real-service configuration is outside this fixture.
