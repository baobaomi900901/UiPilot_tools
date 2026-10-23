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
