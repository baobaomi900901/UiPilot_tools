# @uipilot/plugin-cli

Validate and pack UiPilot public plugins locally without installing UiPilot or Rust.

The package version is `0.1.0`; its current validation target is UiPilot Host `0.8.0`, API v1. Windows bearer credential references require `minimumHostVersion >= 0.4.0` and exact declared secret/host bindings; the CLI validates metadata and never reads stored credentials. Historical feature minima remain unchanged.

The package requires Node.js 20 or newer. Install the supplied npm tarball:

```text
npm install --global ./uipilot-plugin-cli-0.1.0.tgz
uipilot-plugin validate <source> [--platform windows|macos] [--json]
uipilot-plugin pack <source-directory> --output <file.uipilot-plugin> [--platform windows|macos] [--json]
```

For example, from your plugin project with a `package/` directory:

```text
uipilot-plugin validate ./package --platform windows
uipilot-plugin pack ./package --output ./com.example.hello.uipilot-plugin --platform windows
uipilot-plugin validate ./com.example.hello.uipilot-plugin --platform windows
```

Then open UiPilot Settings > Plugins > Public plugins and select the plugin archive for local installation and interaction checks. CLI checks do not install plugins, execute Runtime/window code, contact UiPilot, use the network, or modify the source. Validation is read-only; pack writes only the requested archive and its temporary file.

Pack requires an explicit output path ending exactly in `.uipilot-plugin`, outside the source directory. Its parent directory must already exist, and its ancestor directories must be ordinary directories without symlinks or Windows reparse points. Existing output files, directories and links are never overwritten; there is no `--force`. Choose a new filename for another build. The output filesystem must support hard links (typically NTFS on Windows); unsupported filesystems such as FAT/exFAT return `OUTPUT_WRITE_FAILED`. Pack uses one validated snapshot and produces deterministic ZIP bytes with a SHA-256 hash.

`--platform` defaults to Windows on Windows and macOS on macOS; other systems require an explicit selection. Exit codes are `0` for valid/created, `1` for an invalid/unsafe/incompatible package or expected output failure, and `2` for invalid CLI usage or an internal CLI failure.

Use `--json` for stable reports. Validate returns `PluginValidationReportV1` unchanged. Pack success returns `{schemaVersion:1,created:true,output:{path,sha256},validation,warnings?}`; an expected failure returns `{schemaVersion:1,created:false,output:{path},validation?,error:{code,message}}`. Pack error codes are `SOURCE_INVALID`, `PACKAGE_INVALID`, `OUTPUT_INVALID`, `OUTPUT_EXISTS`, and `OUTPUT_WRITE_FAILED`. A `TEMP_CLEANUP_FAILED` warning means the archive was created successfully but its temporary file could not be removed; the exit code remains `0`. Usage/internal errors use `CLI_USAGE`/`CLI_INTERNAL` and exit `2`.

`timer.control` is accepted only for a Windows `submit + window` plugin with a window entry, `ui.window`, `notifications.publish`, and the fixed validated `assets/sounds/timer-alarm.wav` resource.

The package is not yet published to the npm Registry. Registry installation instructions will be added only after publication. These commands support local packaging, validation and installation, without online publishing.


The 0.5.0 Host target accepts both legacy `command` packages and new `commands` arrays (1–16 mainResult commands, including a one-item array). Commands packages require minimumHostVersion >= 0.5.0. The offline multi demo is included in reference-package verification; isolated consumer smoke compares its freshly packed bytes and SHA-256 to the tracked Rust staging fixture. This is local static release verification; actual Host interaction and final independent release review are separate gates.

New Hosts support old packages and validated package rollback. After a normal transaction or required recovery writes internal state schema 2, older schema-1-only Host binaries are not guaranteed to read that data directory. CLI validation does not migrate Host state or promise binary downgrade compatibility. Unified actions/onAction, per-command shortcuts, independent alias collections, multi-command panel/window and M3 remain outside M2-A.
