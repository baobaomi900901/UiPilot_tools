"""Interactive UiPilot packager. Python 3.10+, standard library only."""
from __future__ import annotations

import codecs
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parent.parent
OVERRIDES = {"CARGO_TARGET_DIR", "CARGO_BUILD_TARGET", "TAURI_CONFIG", "TAURI_CONFIG_PATH",
             "TAURI_BUILD_TARGET", "CARGO_ENCODED_RUSTFLAGS", "RUSTFLAGS"}
VERSION = r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?"


def read_json(path):
    return json.loads(Path(path).read_text(encoding="utf-8-sig"))


def write_json(path, value):
    Path(path).write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding="utf-8")


def hash_file(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def is_reparse(path):
    info = Path(path).lstat()
    return stat.S_ISLNK(info.st_mode) or bool(getattr(info, "st_file_attributes", 0) & 0x400)


def ordinary_directory(path):
    path = Path(os.path.abspath(path))  # Do not resolve away junctions before checking.
    for entry in (path, *path.parents):
        if os.path.lexists(entry) and (is_reparse(entry) or not entry.is_dir()):
            raise ValueError(f"输出路径必须是普通目录，不能包含链接：{entry}")
    return path


def manifest(source):
    value = read_json(Path(source) / "plugin.json")
    if not isinstance(value, dict):
        raise ValueError("plugin.json 必须是 JSON 对象")
    plugin_id, version = value.get("pluginId", ""), value.get("version", "")
    if not isinstance(plugin_id, str) or not re.fullmatch(r"[A-Za-z0-9](?:[A-Za-z0-9._-]*[A-Za-z0-9_-])?", plugin_id):
        raise ValueError("无效插件 ID")
    if re.match(r"^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)", plugin_id, re.I):
        raise ValueError("插件 ID 是保留文件名")
    if not isinstance(version, str) or not re.fullmatch(VERSION, version):
        raise ValueError("无效插件版本")
    return value


def new_run(root, mode, source=None):
    root = Path(root).absolute()
    if mode not in ("setup", "plugin"):
        raise ValueError("Invalid package mode")
    output = ordinary_directory(root / "artifacts/packages/.runs")
    if mode == "plugin":
        source = Path(source).absolute()
        manifest(source)
        if output.is_relative_to(source):
            raise ValueError("Output must be outside plugin source")
    config = read_json(root / "src-tauri/tauri.conf.json")
    if not re.fullmatch(VERSION, config["version"]) or not re.fullmatch(r"[A-Za-z0-9_-]+", config["productName"]):
        raise ValueError("Invalid product filename")
    run_id = time.strftime("%Y%m%d-%H%M%S-") + uuid.uuid4().hex
    directory = output / f"{mode}-{run_id}"
    directory.mkdir(parents=True)
    ordinary_directory(directory)
    return {"schemaVersion": 1, "runId": run_id, "mode": mode, "root": str(root),
            "source": str(source) if source else None, "runDirectory": str(directory),
            "version": config["version"], "productName": config["productName"]}


def cancelled(req):
    if (Path(req["runDirectory"]) / "cancel.flag").exists():
        raise RuntimeError("Packaging cancelled")


def log(req, text):
    with (Path(req["runDirectory"]) / "run.log").open("a", encoding="utf-8") as stream:
        stream.write(text + "\n")


def build_environment():
    env = {key: value for key, value in os.environ.items() if key.upper() not in OVERRIDES}
    env["PYTHONIOENCODING"] = "utf-8"
    return env


def tools_for(req):
    root = Path(req["root"])
    if not (root / "node_modules/.bin/tauri.cmd").is_file():
        raise ValueError("缺少项目依赖，请先在项目目录运行 npm ci")
    node, npm = shutil.which("node.exe"), shutil.which("npm.cmd")
    if not node or not npm:
        raise ValueError("请安装 Node.js 和 npm")
    npm_cli = Path(npm).parent / "node_modules/npm/bin/npm-cli.js"
    if not npm_cli.is_file():
        raise ValueError("npm-cli.js 缺失，请修复 Node.js/npm 安装")
    if req["mode"] == "setup":
        if os.environ.get("PROCESSOR_ARCHITECTURE", "").upper() != "AMD64":
            raise ValueError("Setup 需要 Windows x64 环境")
        if not shutil.which("cargo.exe") or not shutil.which("rustc.exe"):
            raise ValueError("请安装仓库要求的 Rust/MSVC 和 C++ 构建工具")
        rust = subprocess.run(["rustc.exe", "-vV"], cwd=root, env=build_environment(),
                              capture_output=True, text=True, creationflags=subprocess.CREATE_NO_WINDOW)
        if rust.returncode or "host: x86_64-pc-windows-msvc" not in rust.stdout:
            raise ValueError("Rust host 必须为 x86_64-pc-windows-msvc")
        for name in ("Everything.exe", "everything.lock.json", "LICENSE.txt", "Everything-UiPilot.ini"):
            if not (root / "src-tauri/resources/everything" / name).is_file():
                raise ValueError(f"缺少 Everything 资源：{name}")
    return node, str(npm_cli)


def run_command(args, req):
    # Explicit executable + argv; never interpolate a source path into a shell command.
    with (Path(req["runDirectory"]) / "run.log").open("ab") as stream:
        return subprocess.run(args, cwd=req["root"], env=build_environment(),
                              stdin=subprocess.DEVNULL, stdout=stream, stderr=subprocess.STDOUT,
                              creationflags=subprocess.CREATE_NO_WINDOW).returncode


def setup_candidate(req):
    return Path(req["root"]) / "src-tauri/target/release/bundle/nsis" / f"{req['productName']}_{req['version']}_x64-setup.exe"


def evidence(path):
    return (path.stat().st_mtime_ns, hash_file(path)) if path.is_file() else None


def build(req, tools, command=run_command):
    node, npm_cli = tools
    root, directory = Path(req["root"]), Path(req["runDirectory"])
    started = time.time()
    if req["mode"] == "setup":
        candidate = setup_candidate(req)
        before = evidence(candidate)
        artifact = directory / candidate.name
        steps = [("构建生产 Setup", [node, npm_cli, "run", "tauri", "--", "build", "--bundles", "nsis"])]
    else:
        value = manifest(req["source"])
        artifact = directory / f"{value['pluginId']}-{value['version']}.uipilot-plugin"
        cli = str(root / "packages/plugin-cli/dist/cli.mjs")
        steps = [("构建插件 CLI", [node, npm_cli, "run", "build", "--workspace", "@uipilot/plugin-cli"]),
                 ("校验插件源码", [node, cli, "validate", req["source"], "--platform", "windows", "--json"]),
                 ("生成插件包", [node, cli, "pack", req["source"], "--output", str(artifact), "--platform", "windows", "--json"]),
                 ("校验插件归档", [node, cli, "validate", str(artifact), "--platform", "windows", "--json"])]
    if artifact.exists():
        raise ValueError("Staging artifact already exists")
    for stage, args in steps:
        cancelled(req)
        log(req, f"[{time.strftime('%H:%M:%S')}] {stage}")
        code = command(args, req)
        cancelled(req)
        if code:
            raise RuntimeError(f"{stage} failed (exit {code}). See run.log.")
    if req["mode"] == "setup":
        if not candidate.is_file() or is_reparse(candidate) or candidate.stat().st_mtime < started - 2 or evidence(candidate) == before:
            raise ValueError("No fresh current-version Setup generated")
        shutil.copyfile(candidate, artifact)
    if not artifact.is_file() or is_reparse(artifact) or not artifact.stat().st_size:
        raise ValueError("Missing or empty artifact")
    digest = hash_file(artifact)
    log(req, f"SHA256 {digest}  {artifact}")
    return {"schemaVersion": 1, "runId": req["runId"], "state": "Success",
            "artifact": str(artifact), "sha256": digest}


def verify_result(req, result):
    cancelled(req)
    if result.get("state") != "Success" or result.get("runId") != req["runId"]:
        raise ValueError(result.get("message", "No successful matching build"))
    artifact = Path(result["artifact"])
    if artifact.parent != Path(req["runDirectory"]) or not artifact.is_file() or is_reparse(artifact):
        raise ValueError("Unexpected artifact path")
    if hash_file(artifact) != result["sha256"]:
        raise ValueError("Artifact hash mismatch")
    return artifact


def publish(req, result):
    source = verify_result(req, result)
    folder = "setup" if req["mode"] == "setup" else "plugins"
    output = ordinary_directory(Path(req["root"]) / "artifacts/packages" / folder)
    output.mkdir(parents=True, exist_ok=True)
    ordinary_directory(output)
    destination = output / source.name
    if os.path.lexists(destination) and (is_reparse(destination) or not destination.is_file()):
        raise ValueError("Output must be an ordinary file")
    temporary = output / f".publish-{uuid.uuid4().hex}.tmp"
    try:
        with source.open("rb") as src, temporary.open("xb") as dst:
            shutil.copyfileobj(src, dst)
        if hash_file(temporary) != result["sha256"]:
            raise ValueError("Published artifact hash mismatch")
        cancelled(req)
        os.replace(temporary, destination)
        return destination
    finally:
        temporary.unlink(missing_ok=True)


def plugins(root):
    entries = []
    for source in sorted((root / "examples/public-plugins").glob("*/package")):
        try:
            entries.append((source, manifest(source)))
        except (OSError, ValueError, KeyError) as error:
            print(f"跳过无效插件 {source}: {error}")
    return entries


def read_choice(count):
    while True:
        try:
            value = input("请输入编号: ").strip()
        except EOFError:
            return 0
        if value.isascii() and value.isdigit() and 0 <= int(value) <= count:
            return int(value)
        print(f"请输入 0 到 {count} 之间的编号。")


def choose_package(root):
    print("\nUiPilot 打包工具\n1. 主程序 Setup 包\n2. 插件\n0. 退出")
    choice = read_choice(2)
    if choice == 0:
        return None
    if choice == 1:
        return "setup", None
    entries = plugins(root)
    if not entries:
        raise ValueError("没有找到可打包插件")
    for index, (_, value) in enumerate(entries, 1):
        print(f"{index}. {value.get('name', value['pluginId'])} ({value['pluginId']}) v{value['version']}")
    print("0. 取消")
    selected = read_choice(len(entries))
    return ("plugin", entries[selected - 1][0]) if selected else None


def worker(request_path):
    req = read_json(request_path)
    directory = ordinary_directory(req["runDirectory"])
    if Path(req["root"]).resolve() != ROOT or request_path.parent != directory or directory.parent != ROOT / "artifacts/packages/.runs":
        raise ValueError("Request does not belong to this repository")
    try:
        result = build(req, tools_for(req))
        cancelled(req)
    except Exception as error:
        log(req, str(error))
        write_json(directory / "result.json", {"runId": req["runId"], "state": "Failed", "message": str(error)})
        return 1
    write_json(directory / "result.json", result)
    return 0


def execute(req, verbose=False):
    from package_job import OwnedWorker
    from package_progress import Progress
    directory = Path(req["runDirectory"])
    request_path = directory / "request.json"
    write_json(request_path, req)
    logfile = directory / "run.log"
    logfile.touch()
    print(f"\n开始打包（Ctrl+C 可取消）。同名产物成功后自动覆盖。\n日志：{logfile}")
    progress = Progress(req['mode'], verbose=verbose)
    try:
        with OwnedWorker([sys.executable, "-u", str(Path(__file__).resolve()), "--worker", str(request_path)], req["root"]) as job:
            decoder = codecs.getincrementaldecoder("utf-8")(errors="replace")
            with logfile.open("rb") as stream:
                while True:
                    code = job.poll()
                    progress.feed(decoder.decode(stream.read(), final=code is not None))
                    progress.tick()
                    if code is not None:
                        break
                    time.sleep(0.1)
        result = read_json(directory / "result.json")
        if code:
            raise RuntimeError(result.get("message", f"Worker failed (exit {code})"))
        artifact = publish(req, result)
        progress.finish(True)
        print(f"\n打包成功：{artifact}\n大小：{artifact.stat().st_size / 1024 / 1024:.2f} MiB\nSHA256：{result['sha256']}")
        return 0
    except KeyboardInterrupt:
        (directory / "cancel.flag").touch()
        write_json(directory / "result.json", {"runId": req["runId"], "state": "Cancelled"})
        print("\n已取消，原有产物保留。")
        return 130
    except Exception as error:
        progress.finish(False)
        print(f"\n打包失败：{error}\n日志目录：{directory}")
        return 1


def main():
    # uv / redirected Windows consoles may otherwise default to GBK. Keep the
    # console and our UTF-8 log consistent, and flush progress as each line arrives.
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, 'reconfigure'):
            stream.reconfigure(encoding='utf-8', errors='replace', line_buffering=True)
    if sys.version_info < (3, 10):
        print("请安装 Python 3.10 或更新版本。")
        return 1
    if os.name != "nt":
        print("当前 Setup 打包流程需要 Windows。")
        return 1
    if len(sys.argv) == 3 and sys.argv[1] == "--worker":
        return worker(Path(sys.argv[2]).absolute())
    parser = argparse.ArgumentParser(description='UiPilot 终端交互打包工具')
    parser.add_argument('--verbose', action='store_true', help='同时显示完整构建输出')
    args = parser.parse_args()
    try:
        selected = choose_package(ROOT)
        return execute(new_run(ROOT, *selected), verbose=args.verbose) if selected else 0
    except (EOFError, KeyboardInterrupt):
        return 0
    except Exception as error:
        print(f"打包失败：{error}")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
