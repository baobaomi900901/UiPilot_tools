# Development startup

Run `python D:\code\.tools\dev-uipilot.py` in a terminal.

1. `是否使用上一次的配置？(Y/n)` — Enter reuses the last project and rust_uia paths. N prompts for both paths. The first run asks for paths directly. Explicit `--root` / `--rust-uia` arguments override the corresponding remembered values and bypass this first question.
2. `是否重新编译并打包 rust_uia 产出物？(y/N)` — Enter reuses the validated Provider generation. Y runs the existing release build and capability probe, then stages a new generation. Missing, damaged or mismatched assets stop with instructions to rebuild; reuse never silently builds rust_uia.
3. The normal Tauri dev wrapper starts Vite and incrementally builds/runs the Rust host.

The machine entry point is maintained at `scripts/dev_uipilot_launcher.py`, installed as `D:\code\.tools\dev-uipilot.py`. It remembers both paths in the adjacent `dev-uipilot.config.json`. The project also retains `rust_uia` in `.uipilot-local.json` for production builds.

Provider source build output: `<rust-uia>\target\uipilot-provider\x86_64-pc-windows-msvc\release\uiautoma-uiautomation-provider.exe`.

Dev copy: `<root>\artifacts\provider-dev\<generation-directory>\UIAutoma.UIAutomation.Provider.exe`, with its integrity/capability manifest. `current.json` points at the last successful generation for the chosen source path. Existing compatible generations can be adopted without compiling. Generation directories remain immutable across subsequent dev starts; old directories are not automatically deleted while a process may be using them.

For automation, `--non-interactive` suppresses prompts and defaults to reuse. Use `--rebuild-provider` to explicitly rebuild or `--reuse-provider` to explicitly reuse. Extra arguments such as `--no-watch` continue to Tauri.

Dev-only startup diagnostics print elapsed times, PID and fixed stage names. While startup is incomplete, a watchdog prints the last stage every 15 seconds. Per-session logs are under `<root>\artifacts\dev-logs`; they contain no selected text, clipboard contents or credentials. “等待主窗口前端连接” means native setup completed but frontend initialization has not completed; it is not evidence that Cargo is still compiling. Warnings about unused Rust code do not by themselves imply a hang.

Production setup/bundle builds still use the owned `provider_build.py build-session` flow and fresh Provider preparation. Dev cache reuse does not alter packaging guarantees.
