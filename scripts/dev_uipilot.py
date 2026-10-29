"""Prepare one Provider generation, then delegate to the existing Tauri dev wrapper."""
from __future__ import annotations
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import uuid
import time
import provider_build as provider

def log(message):
    print(f'[{time.strftime("%H:%M:%S")}] [UiPilot dev] {message}', flush=True)

def owned_dev(args, *, cwd, env):
    from package_job import OwnedWorker
    node=shutil.which('node.exe')
    npm_cli=Path(args[0]).parent/'node_modules/npm/bin/npm-cli.js'
    if not node or not npm_cli.is_file(): raise ValueError('Node/npm installation is incomplete')
    previous=os.environ.copy()
    try:
        os.environ.clear(); os.environ.update(env)
        worker=OwnedWorker([node,str(npm_cli),*args[1:]],cwd,inherit_stdio=True)
    finally:
        os.environ.clear(); os.environ.update(previous)
    with worker:
        while worker.poll() is None: time.sleep(.1)
        return type('Result',(),{'returncode':worker.poll()})()

def run_dev(root, rust_uia, extra=(), runner=subprocess.run):
    root=Path(root).resolve()
    started=time.monotonic()
    log(f'项目：{root}')
    log(f'正在编译并校验截图 Provider：{Path(rust_uia).resolve()}')
    stage=root/'artifacts/provider-dev'/uuid.uuid4().hex
    exe=provider.prepare(rust_uia,stage)
    manifest=provider.validate(stage)
    log(f'Provider 已就绪（{time.monotonic()-started:.1f} 秒）')
    env=dict(os.environ,UIPILOT_SCREENSHOT_PROVIDER_EXE=str(exe.resolve()),UIPILOT_PROVIDER_PREPARED='1',
             UIPILOT_PROVIDER_GENERATION=manifest['generation'])
    npm=shutil.which('npm.cmd') or shutil.which('npm')
    if not npm: raise ValueError('npm is required')
    if runner is subprocess.run and os.name=='nt': runner=owned_dev
    log('正在启动 Tauri 开发会话：接下来启动前端、编译 Rust 主程序，请留意下方日志。')
    log('主程序初始化完成后会显示“主程序已就绪”；Ctrl+C 停止本次开发会话。')
    result=runner([npm,'run','tauri','--','dev',*extra],cwd=root,env=env).returncode
    log(f'开发会话已退出，退出码 {result}（总计 {time.monotonic()-started:.1f} 秒）')
    return result

def main(argv=None):
    parser=argparse.ArgumentParser()
    parser.add_argument('--root',type=Path,default=provider.ROOT)
    parser.add_argument('--rust-uia',type=Path,default=None)
    args,extra=parser.parse_known_args(argv)
    if args.rust_uia is not None:
        config=args.root.resolve()/'.uipilot-local.json'
        saved=json.loads(config.read_text(encoding='utf-8')) if config.is_file() else {}
        saved['rust_uia']=str(args.rust_uia.resolve())
        config.write_text(json.dumps(saved,indent=2),encoding='utf-8')
    try: return run_dev(args.root,args.rust_uia or provider.source_root(args.root),extra)
    except KeyboardInterrupt:
        log('开发会话已停止。')
        return 130
    except (ValueError, OSError, RuntimeError) as error:
        log(f'启动失败：{error}')
        return 1

if __name__=='__main__': raise SystemExit(main())
