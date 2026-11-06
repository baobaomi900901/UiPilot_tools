"""Read-only preflight for the Windows side of the Apple Silicon candidate.

Use Python 3.10+ with -B. No dependency installation, compiler invocation,
Provider staging, application launch, machine changes or network checks.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import zipfile

sys.dont_write_bytecode = True
CANDIDATE = "a79f612f99c31b4bb081fbdb04eb71ac92b7c761"
SOURCE_ZIP_SHA256 = "47970c7579ee4c381afc90092d620122f84d67760eaead827ebe19b7d75ff673"
HOST = "x86_64-pc-windows-msvc"


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def command(args, cwd):
    # Only stdout of version probes and Git identity checks is consumed. Never
    # forward command errors, absolute paths, configuration or environment.
    env = os.environ.copy()
    env.pop("NODE_OPTIONS", None)
    env.update(GIT_OPTIONAL_LOCKS="0", GIT_TERMINAL_PROMPT="0", RUSTUP_AUTO_INSTALL="0")
    try:
        result = subprocess.run(args, cwd=cwd, env=env, capture_output=True, timeout=15,
                                stdin=subprocess.DEVNULL, text=True, errors="replace", check=False)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return result.stdout.strip() if result.returncode == 0 else None


def git_identity(root):
    head = command(["git", "-C", str(root), "rev-parse", "--verify", "HEAD"], root)
    status = command(["git", "--no-optional-locks", "-c", "core.fsmonitor=false", "-C", str(root),
                      "status", "--porcelain=v1", "--untracked-files=normal"], root)
    return head if head and re.fullmatch(r"[0-9a-f]{40}", head) and status == "" else None


def matches_archive(root, archive):
    if sha256(archive) != SOURCE_ZIP_SHA256:
        return False
    with zipfile.ZipFile(archive) as source:
        if source.comment.decode("ascii") != CANDIDATE:
            return False
        expected = {item.filename.removeprefix("UiPilot_tools/"): item
                    for item in source.infolist() if not item.is_dir()}
        found = set()
        for directory, dirs, names in os.walk(root, followlinks=False):
            for name in dirs + names:
                path = Path(directory, name)
                info = path.lstat()
                if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT:
                    return False
            for name in names:
                path = Path(directory, name)
                relative = path.relative_to(root).as_posix()
                if relative == ".uipilot-local.json":
                    continue
                item = expected.get(relative)
                if item is None or path.stat().st_size != item.file_size or path.read_bytes() != source.read(item):
                    return False
                found.add(relative)
        return found == expected.keys()


class Report:
    def __init__(self):
        self.failures = 0

    def check(self, label, passed, success, remedy):
        if not passed:
            self.failures += 1
        print(f"{'PASS' if passed else 'BLOCKED'} {label} - {success if passed else remedy}")
        return bool(passed)


def windows_native_x64():
    if os.name != "nt":
        return False
    process_machine, host_machine = ctypes.c_ushort(), ctypes.c_ushort()
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    try:
        check = kernel.IsWow64Process2
    except AttributeError:
        return False
    check.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_ushort), ctypes.POINTER(ctypes.c_ushort)]
    return bool(check(ctypes.c_void_p(-1), ctypes.byref(process_machine), ctypes.byref(host_machine))
                and host_machine.value == 0x8664 and process_machine.value == 0)


def rust_binary(name, pin):
    executable = shutil.which(name + ".exe")
    if not executable:
        return None
    rustup = shutil.which("rustup.exe")
    if rustup and Path(executable).parent == Path(rustup).parent:
        # A rustup proxy can install a missing channel on a mere version query.
        # Probe only an already installed pinned binary; never call the proxy.
        home = Path(os.environ.get("RUSTUP_HOME", Path.home() / ".rustup"))
        binary = home / "toolchains" / f"{pin}-{HOST}" / "bin" / (name + ".exe")
        return str(binary) if binary.is_file() else None
    return executable


def check_tools(root, report):
    node = shutil.which("node.exe")
    info = command([node, "-p", "process.arch+' '+process.versions.node"], root) if node else None
    match = re.fullmatch(r"x64 (\d+)\.(\d+)\.(\d+)", info or "")
    ok = match and (int(match[1]) == 22 and int(match[2]) >= 13 or int(match[1]) >= 24)
    report.check("Node", ok, info, "使用原生 x64 Node 22.13+ 的 22.x，或 24+；本轮 Mac 使用 22.22.1。")
    npm = shutil.which("npm.cmd")
    report.check("npm.cmd", npm and (Path(npm).parent / "node_modules/npm/bin/npm-cli.js").is_file(),
                 "npm CLI 已找到", "安装相应 npm 并修正 PATH。")
    toolchain = (root / "rust-toolchain.toml").read_text(encoding="utf-8")
    pin = re.search(r'(?m)^channel\s*=\s*"([0-9.]+)"\s*$', toolchain).group(1)
    rustc, cargo = (rust_binary(name, pin) for name in ("rustc", "cargo"))
    rust = command([rustc, "-vV"], root) if rustc else None
    override = os.environ.get("RUSTUP_TOOLCHAIN", "")
    report.check("Rust", rust and f"host: {HOST}" in rust.splitlines() and f"release: {pin}" in rust.splitlines()
                 and override in ("", pin, f"{pin}-{HOST}"), f"{pin} {HOST}",
                 f"准备已安装的 {pin}-{HOST}；移除冲突的 Rust 工具链覆盖。")
    cargo_version = command([cargo, "-V"], root) if cargo else None
    report.check("Cargo", cargo_version and cargo_version.startswith(f"cargo {pin} "), pin, "准备与 Rust 同版本的 Cargo。")
    cl, linker = (shutil.which(name) for name in ("cl.exe", "link.exe"))
    report.check("MSVC x64", cl and linker and Path(cl).parent == Path(linker).parent
                 and os.environ.get("VSCMD_ARG_TGT_ARCH", "").lower() in ("x64", "amd64"),
                 "x64 编译器与链接器已就绪", "从 VS x64 Native Tools 终端运行；安装 C++ 桌面工作负载。")
    report.check("Windows SDK", all(shutil.which(name) for name in ("rc.exe", "mt.exe")), "rc/mt 已找到",
                 "在同一开发者终端初始化 Windows SDK。")
    ps = shutil.which("powershell.exe")
    version = command([ps, "-NoProfile", "-NonInteractive", "-Command",
                       "$PSVersionTable.PSVersion.ToString()"], root) if ps else None
    report.check("Windows PowerShell", re.fullmatch(r"5\.1(?:\.\d+)*", version or ""),
                 "5.1", "准备 Windows PowerShell 5.1 的 powershell.exe。")


def webview2_registered():
    import winreg
    for hive in (winreg.HKEY_LOCAL_MACHINE, winreg.HKEY_CURRENT_USER):
        for view in (winreg.KEY_WOW64_32KEY, winreg.KEY_WOW64_64KEY):
            try:
                with winreg.OpenKey(hive, r"SOFTWARE\Microsoft\EdgeUpdate\Clients", 0, winreg.KEY_READ | view) as clients:
                    for index in range(winreg.QueryInfoKey(clients)[0]):
                        try:
                            with winreg.OpenKey(clients, winreg.EnumKey(clients, index)) as product:
                                name = winreg.QueryValueEx(product, "name")[0]
                                version = winreg.QueryValueEx(product, "pv")[0]
                                if isinstance(name, str) and "webview2" in name.lower() and re.fullmatch(r"[1-9][0-9.]*(?:[0-9])?", str(version)):
                                    return True
                        except OSError:
                            continue
            except OSError:
                continue
    return False


def check_provider(root, report, expected_revision):
    # Called only after verifying the candidate source. Use its own resolution
    # precedence without printing the path or running the Provider.
    sys.path.insert(0, str(root / "scripts"))
    try:
        from provider_build import source_root
        provider = source_root(root)
        revision = None
        if not provider.is_relative_to(root) and all((provider / name).is_file() for name in ("Cargo.toml", "Cargo.lock")):
            revision = git_identity(provider)
    except (OSError, ValueError, TypeError, AttributeError, ImportError):
        revision = None
    finally:
        sys.path.pop(0)
    report.check("Provider", revision and (expected_revision is None or expected_revision == revision),
                 "独立干净源码 revision " + (revision or ""),
                 "备好独立 Provider 的干净 Git checkout、Cargo.toml、Cargo.lock 与指定 revision；设置 UIPILOT_RUST_UIA_ROOT。")


def check_resources(root, report):
    folder = root / "src-tauri/resources/everything"
    lock = json.loads((folder / "everything.lock.json").read_text(encoding="utf-8"))
    report.check("Everything.exe", sha256(folder / "Everything.exe") == lock["everythingExeSha256"],
                 f"x64 {lock['version']} SHA-256 匹配", "从完整候选源码恢复 Everything.exe。")
    # The lock describes upstream CRLF license bytes; Git can check out LF.
    license_bytes = (folder / "LICENSE.txt").read_bytes().replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")
    report.check("Everything 许可证", hashlib.sha256(license_bytes).hexdigest() == lock["licenseSha256"],
                 "许可证文本匹配", "从候选源码恢复许可证。")
    config = json.loads((root / "src-tauri/tauri.windows.conf.json").read_text(encoding="utf-8"))
    nsis = config["bundle"]["windows"]["nsis"]
    paths = [name for name in config["bundle"]["resources"] if not name.startswith("resources/provider/")]
    paths += [nsis["installerHooks"], nsis["template"]]
    report.check("Windows 打包输入", all((root / "src-tauri" / path).is_file() for path in paths),
                 "共享声音、Everything、NSIS 模板与 hooks 齐备", "从候选源码恢复所缺共享资源与 Windows 配置。")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, required=True, help="candidate source directory")
    parser.add_argument("--source-zip", type=Path, help="original handoff ZIP for a freshly extracted, non-Git candidate")
    parser.add_argument("--provider-commit", help="optional independently approved Provider revision")
    args = parser.parse_args(argv)
    if args.provider_commit is not None and not re.fullmatch(r"[0-9a-f]{40}", args.provider_commit):
        parser.error("--provider-commit requires a full lowercase Git SHA")
    root = args.repo.resolve()
    report = Report()
    try:
        source_ok = matches_archive(root, args.source_zip) if args.source_zip else git_identity(root) == CANDIDATE
    except (OSError, ValueError, UnicodeError, zipfile.BadZipFile):
        source_ok = False
    if not report.check("候选源码", source_ok, CANDIDATE, "使用固定候选的干净 Git checkout，或重新解压原 ZIP 后传 --source-zip。"):
        return 2
    try:
        check_resources(root, report)
    except (OSError, ValueError, TypeError, KeyError):
        report.check("静态输入", False, "", "候选资源或锁文件丢失或不可读。")
    if not report.check("Windows x64", windows_native_x64(), "原生 x64 Windows / Python",
                        "在原生 Windows x64 的 64 位 Python 终端执行此检查。"):
        return 2
    report.check("Python", sys.version_info >= (3, 10), sys.version.split()[0], "安装原生 Python 3.10+。")
    check_tools(root, report)
    git_version = command(["git", "--version"], root) or ""
    report.check("Git", re.fullmatch(r"git version [0-9.]+(?:[a-zA-Z0-9.-]*)", git_version), "可读取 revision", "安装 Git for Windows 并修正 PATH。")
    check_provider(root, report, args.provider_commit)
    print("INFO WebView2 - " + ("已找到 EdgeUpdate 注册的运行时，仍须验证真实启动。"
                               if webview2_registered() else "未找到已注册的运行时，请在安装实测中核对。"))
    print("INFO NSIS - " + ("PATH 可见 makensis.exe。" if shutil.which("makensis.exe") else
                           "PATH 未发现 makensis.exe；Tauri 可以使用其缓存或在获准构建时下载。"))
    print("BUILD GATE: 如需首次下载 Cargo/npm/NSIS 工具，须由设备方准备允许的缓存或网络。"
          "由受管 Tauri 构建生成 Provider、Helper 和安装包；WebView2 运行时另在安装实测确认。")
    return 2 if report.failures else 0


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    raise SystemExit(main())
