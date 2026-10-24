# Deterministic CLI pack fixture

`roundtrip.uipilot-plugin` contains only synthetic data: a fixed manifest, one runtime file,
an empty directory, and a nested empty directory. It uses the production stored ZIP writer.

The exact source snapshot is `fixtureSnapshot()` in
`packages/plugin-cli/tests/archive-writer.test.ts`. To regenerate from the repository root
in PowerShell:

```powershell
$env:UPDATE_PACK_FIXTURE = '1'
npm test --workspace=@uipilot/plugin-cli -- archive-writer.test.ts
Remove-Item Env:UPDATE_PACK_FIXTURE
```

Normal tests only read and compare the fixture byte for byte. The Rust
`cli_pack_fixture_stages_empty_and_nested_directories` test passes these bytes through
`package::stage`, checks files and empty/nested directories, and never installs a plugin.
The `.uipilot-plugin` suffix is globally ignored; stage this fixture explicitly with
`git add -f tests/fixtures/public-plugin-pack/roundtrip.uipilot-plugin` when regenerating.


`multi-command.uipilot-plugin` is the exact production Host-target-0.5.0 CLI pack of
`examples/public-plugins/com.uipilot.demo-multi/package/`. It is generated intentionally
with a fresh CLI build and committed as release data; normal tests never rewrite it.
The Rust `cli_multi_command_pack_fixture_stages_on_0_5_0` test uses include_bytes! to
stage these exact bytes in a fresh TestRoot, compares every package resource and both
ordered commands, and rejects a historical 0.4 Host. The isolated offline consumer
smoke freshly builds/installs the CLI and compares its new output byte-for-byte and by
SHA-256 with this file before cleaning its temporary directory. Missing inputs or byte
drift fail verification. Root verification deliberately runs Rust before CLI smoke.
Regenerate only as an intentional reviewed release change using a fresh production CLI
`pack` into a unique temporary output, then copy that output here; do not use the older
roundtrip fixture's update flag. Explicitly stage only this ignored archive with
`git add -f tests/fixtures/public-plugin-pack/multi-command.uipilot-plugin`.
