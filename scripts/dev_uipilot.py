"""Prepare or reuse a validated Provider, then run the existing Tauri dev wrapper."""
from __future__ import annotations
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import uuid
import time
import provider_build as provider


def log(message):
    print(f'[{time.strftime("%H:%M:%S")}] [UiPilot dev] {message}', flush=True)


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(mode='w', encoding='utf-8', dir=path.parent, delete=False) as file:
        temporary = Path(file.name)
        json.dump(value, file, ensure_ascii=False, indent=2)
    try:
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def prepared_provider(root, source, rebuild=False):
    source = Path(source).resolve()
    cache = Path(root).resolve() / 'artifacts/provider-dev'
    receipt = cache / 'current.json'
    if rebuild:
        stage = cache / uuid.uuid4().hex
        log(f'重新编译并校验截图 Provider：{source}')
        exe = provider.prepare(source, stage)
        manifest = provider.validate(stage)
        write_json(receipt, {'source': str(source), 'stage': stage.name, 'generation': manifest['generation']})
        return exe, manifest
    if receipt.is_file():
        try:
            saved = json.loads(receipt.read_text(encoding='utf-8'))
            stage_name = saved['stage']
            if not isinstance(stage_name, str) or len(stage_name) != 32 or any(c not in '0123456789abcdef' for c in stage_name):
                raise ValueError('缓存目录名称无效')
            if Path(saved['source']).resolve() != source:
                raise ValueError('缓存来自不同的 rust_uia 路径')
            stage = cache / stage_name
            manifest = provider.validate(stage)
            if saved['generation'] != manifest['generation']:
                raise ValueError('缓存批次不一致')
        except (KeyError, TypeError, OSError, ValueError) as error:
            raise ValueError(f'Provider 缓存不可用：{error}；请选择 Y 或使用 --rebuild-provider') from error
    else:
        # Migrate a previously prepared dev generation, without Cargo or a live probe.
        candidates = sorted(cache.glob('*/asset-manifest.json'), key=lambda p: p.stat().st_mtime, reverse=True)
        stage = None
        for candidate in candidates:
            if len(candidate.parent.name) != 32 or any(c not in '0123456789abcdef' for c in candidate.parent.name):
                continue
            try:
                manifest = provider.validate(candidate.parent)
                build_args = manifest['build_args']
                target = Path(build_args[build_args.index('--target-dir') + 1]).resolve()
                if target != source / 'target/uipilot-provider':
                    continue
                stage = candidate.parent
                break
            except (ValueError, KeyError, TypeError, IndexError, OSError):
                continue
        if stage is None:
            raise ValueError('没有此 rust_uia 路径的可用产物；首次运行请选择 Y 或使用 --rebuild-provider')
        write_json(receipt, {'source': str(source), 'stage': stage.name, 'generation': manifest['generation']})
    log(f'复用 Provider（不编译）：{stage / provider.EXE}')
    return stage / provider.EXE, manifest


def owned_dev(args, *, cwd, env):
    from package_job import OwnedWorker
    node = shutil.which('node.exe')
    npm_cli = Path(args[0]).parent / 'node_modules/npm/bin/npm-cli.js'
    if not node or not npm_cli.is_file():
        raise ValueError('Node/npm installation is incomplete')
    previous = os.environ.copy()
    try:
        os.environ.clear(); os.environ.update(env)
        worker = OwnedWorker([node, str(npm_cli), *args[1:]], cwd, inherit_stdio=True)
    finally:
        os.environ.clear(); os.environ.update(previous)
    with worker:
        while worker.poll() is None:
            time.sleep(.1)
        return type('Result', (), {'returncode': worker.poll()})()


def run_dev(root, rust_uia, extra=(), runner=subprocess.run, *, rebuild=False):
    root = Path(root).resolve()
    started = time.monotonic()
    log(f'项目：{root}')
    exe, manifest = prepared_provider(root, rust_uia, rebuild)
    log(f'Provider 已就绪（{time.monotonic()-started:.1f} 秒）')
    log_directory = root / 'artifacts/dev-logs'
    log_directory.mkdir(parents=True, exist_ok=True)
    startup_log = log_directory / f'{time.strftime("%Y%m%d-%H%M%S")}-{uuid.uuid4().hex[:8]}.log'
    env = dict(os.environ, UIPILOT_SCREENSHOT_PROVIDER_EXE=str(exe.resolve()), UIPILOT_PROVIDER_PREPARED='1',
               UIPILOT_PROVIDER_GENERATION=manifest['generation'], UIPILOT_DEV_STARTUP_LOG=str(startup_log))
    npm = shutil.which('npm.cmd') or shutil.which('npm')
    if not npm:
        raise ValueError('npm is required')
    if runner is subprocess.run and os.name == 'nt':
        runner = owned_dev
    log(f'启动诊断日志：{startup_log}')
    log('正在启动前端、增量编译 Rust 主程序；Provider 的复用不会跳过前后端构建。')
    log('主程序初始化完成后会显示“主程序已就绪”；Ctrl+C 停止本次开发会话。')
    result = runner([npm, 'run', 'tauri', '--', 'dev', *extra], cwd=root, env=env).returncode
    log(f'开发会话已退出，退出码 {result}（总计 {time.monotonic()-started:.1f} 秒）')
    return result


def main(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument('--root', type=Path, default=provider.ROOT)
    parser.add_argument('--rust-uia', type=Path, default=None)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--rebuild-provider', action='store_true')
    mode.add_argument('--reuse-provider', action='store_true')
    parser.add_argument('--non-interactive', action='store_true')
    args, extra = parser.parse_known_args(argv)
    try:
        root = args.root.resolve()
        if not (root / 'src-tauri/tauri.conf.json').is_file():
            raise ValueError(f'UiPilot 项目路径无效：{root}')
        source = (args.rust_uia or provider.source_root(root)).resolve()
        if args.rust_uia is not None:
            config = root / '.uipilot-local.json'
            saved = json.loads(config.read_text(encoding='utf-8')) if config.is_file() else {}
            saved['rust_uia'] = str(source)
            write_json(config, saved)
        rebuild = args.rebuild_provider
        if not (args.rebuild_provider or args.reuse_provider or args.non_interactive) and sys.stdin.isatty():
            while True:
                answer = input('是否重新编译并打包 rust_uia 产出物？(y/N) ').strip().lower()
                if answer in ('', 'n', 'no', 'y', 'yes'):
                    rebuild = answer in ('y', 'yes')
                    break
                print('请输入 Y 或 N。')
        return run_dev(root, source, extra, rebuild=rebuild)
    except KeyboardInterrupt:
        log('开发会话已停止。')
        return 130
    except (ValueError, OSError, RuntimeError, subprocess.SubprocessError, EOFError) as error:
        log(f'启动失败：{error}')
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
