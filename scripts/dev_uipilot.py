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
    stage=root/'artifacts/provider-dev'/uuid.uuid4().hex
    exe=provider.prepare(rust_uia,stage)
    manifest=provider.validate(stage)
    env=dict(os.environ,UIPILOT_SCREENSHOT_PROVIDER_EXE=str(exe.resolve()),UIPILOT_PROVIDER_PREPARED='1',
             UIPILOT_PROVIDER_GENERATION=manifest['generation'])
    npm=shutil.which('npm.cmd') or shutil.which('npm')
    if not npm: raise ValueError('npm is required')
    if runner is subprocess.run and os.name=='nt': runner=owned_dev
    return runner([npm,'run','tauri','--','dev',*extra],cwd=root,env=env).returncode

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
    except KeyboardInterrupt: return 130

if __name__=='__main__': raise SystemExit(main())
