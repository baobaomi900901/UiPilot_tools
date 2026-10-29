"""Build the independent screenshot Provider; never consume a fallback executable."""
from __future__ import annotations
import argparse
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import struct
import sys
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parent.parent
TARGET = 'x86_64-pc-windows-msvc'
EXE = 'UIAutoma.UIAutomation.Provider.exe'
MANIFEST = 'asset-manifest.json'
BUILD_ARGS = ['cargo', 'build', '--release', '--locked', '--target', TARGET]

def source_root(root=ROOT):
    config = Path(root) / '.uipilot-local.json'
    saved = json.loads(config.read_text(encoding='utf-8')) if config.is_file() else {}
    return Path(os.environ.get('UIPILOT_RUST_UIA_ROOT', saved.get('rust_uia', 'D:/code/rust_uia'))).resolve()

def run(args, *, cwd, capture=False):
    env = os.environ.copy()
    for key in ('CARGO_TARGET_DIR','CARGO_BUILD_TARGET','RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS'):
        env.pop(key, None)
    result = subprocess.run(args, cwd=cwd, env=env, check=True, text=True,
                            stdout=subprocess.PIPE if capture else None)
    return result.stdout or ''

@contextmanager
def locked(path):
    """OS-owned lock is released even when a build process crashes."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('a+b') as stream:
        stream.seek(0); stream.write(b'0'); stream.flush(); stream.seek(0)
        deadline = time.monotonic()+300
        if os.name == 'nt':
            import msvcrt
            while True:
                try:
                    msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1); break
                except OSError:
                    if time.monotonic()>deadline: raise TimeoutError('Provider build lock timed out')
                    time.sleep(.1)
            try: yield
            finally:
                stream.seek(0); msvcrt.locking(stream.fileno(), msvcrt.LK_UNLCK, 1)
        else:
            import fcntl
            fcntl.flock(stream, fcntl.LOCK_EX)
            try: yield
            finally: fcntl.flock(stream, fcntl.LOCK_UN)

def digest(path):
    with path.open('rb') as stream:
        h=hashlib.sha256()
        for chunk in iter(lambda: stream.read(1024*1024),b''): h.update(chunk)
        return h.hexdigest()

def validate_pe_x64(path):
    """Check the executable's actual COFF machine and PE32+ optional header."""
    with Path(path).open('rb') as stream:
        header=stream.read(64)
        if len(header)!=64 or header[:2]!=b'MZ': raise ValueError('Provider is not a PE executable')
        offset=struct.unpack_from('<I',header,60)[0]
        if offset<64 or offset>Path(path).stat().st_size-26: raise ValueError('Invalid Provider PE offset')
        stream.seek(offset); pe=stream.read(26)
        if pe[:4]!=b'PE\0\0' or struct.unpack_from('<H',pe,4)[0]!=0x8664 or struct.unpack_from('<H',pe,24)[0]!=0x20b:
            raise ValueError('Provider PE must be x64 PE32+')


def session_receipt(root=ROOT):
    token=os.environ.get('UIPILOT_PROVIDER_BUILD_SESSION','')
    if len(token)!=32 or any(c not in '0123456789abcdef' for c in token):
        raise ValueError('Production bundle requires npm run tauri -- build (owned build session)')
    receipt=Path(root)/'artifacts/provider-build'/f'{token}.json'
    value=json.loads(receipt.read_text(encoding='utf-8'))
    if value.get('token')!=token: raise ValueError('Provider build session receipt mismatch')
    return receipt,value


def build_session(root, args, runner=None):
    """Hold the production resource lock until the whole native CLI exits."""
    root=Path(root).resolve()
    if runner is None: runner=owned_cli
    with locked(root/'artifacts/provider-build/session.lock'):
        token=uuid.uuid4().hex
        receipt=root/'artifacts/provider-build'/f'{token}.json'
        receipt.write_text(json.dumps({'token':token}),encoding='utf-8')
        env=dict(os.environ,UIPILOT_PROVIDER_BUILD_SESSION=token)
        try:
            if args[:1] == ['bundle']:
                destination=root/'src-tauri/resources/provider'
                prepare(source_root(root),destination)
                receipt.write_text(json.dumps({'token':token,'generation':validate(destination)['generation']}),encoding='utf-8')
            # owned_dev accepts Node/npm arguments; invoke native CLI directly to avoid wrapper recursion.
            node=shutil.which('node.exe') or shutil.which('node')
            command=[node,str(root/'node_modules/@tauri-apps/cli/tauri.js'),*args]
            return runner(command,cwd=root,env=env).returncode
        finally: receipt.unlink(missing_ok=True)


def owned_cli(args, *, cwd, env):
    from package_job import OwnedWorker
    previous=os.environ.copy()
    try:
        os.environ.clear(); os.environ.update(env)
        worker=OwnedWorker(args,cwd,inherit_stdio=True)
    finally:
        os.environ.clear(); os.environ.update(previous)
    with worker:
        while worker.poll() is None: time.sleep(.1)
        return type('Result',(),{'returncode':worker.poll()})()

def ping(exe):
    """Bounded probe of our own short-lived process and unique named pipe."""
    name = 'uipilot-build-'+uuid.uuid4().hex
    child = subprocess.Popen([str(exe),'--pipe-name', '\\\\.\\pipe\\'+name,'--parent-pid',str(os.getpid())],
                             stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,
                             creationflags=getattr(subprocess,'CREATE_NO_WINDOW',0))
    script = r'''
$ErrorActionPreference='Stop'
$p=[IO.Pipes.NamedPipeClientStream]::new('.', $env:UIPILOT_PROBE_PIPE, [IO.Pipes.PipeDirection]::InOut)
try {
$p.Connect(5000)
$w=[IO.StreamWriter]::new($p,[Text.UTF8Encoding]::new($false),1024,$true); $w.AutoFlush=$true
$w.WriteLine('{"jsonrpc":"2.0","id":1,"method":"system.ping","params":{}}')
$r=[IO.StreamReader]::new($p,[Text.UTF8Encoding]::new($false),$false,1024,$true)
$t=$r.ReadLineAsync(); if(-not $t.Wait(5000)){throw 'ping timeout'}; $t.Result
} finally {$p.Dispose()}
'''
    try:
        env=dict(os.environ,UIPILOT_PROBE_PIPE=name)
        result=subprocess.run(['powershell.exe','-NoProfile','-NonInteractive','-Command',script],
                              env=env,capture_output=True,text=True,check=True,timeout=12,
                              creationflags=getattr(subprocess,'CREATE_NO_WINDOW',0))
        if len(result.stdout)>65536: raise ValueError('Provider ping response too large')
        reply=json.loads(result.stdout)
        if reply.get('id')!=1 or reply.get('result',{}).get('provider_pid')!=child.pid:
            raise ValueError('Provider ping identity mismatch')
        caps=reply['result']['capabilities']
        if caps.get('point_probe_v1') is not True: raise ValueError('Provider lacks point_probe_v1')
        return caps
    finally:
        if child.poll() is None: child.terminate()
        try: child.wait(timeout=5)
        except subprocess.TimeoutExpired: child.kill(); child.wait()

def validate(directory):
    directory=Path(directory)
    try:
        value=json.loads((directory/MANIFEST).read_text(encoding='utf-8'))
        artifact=value['artifact']; exe=directory/EXE
        if (value['schema_version']!=1 or value['target']!=TARGET or
            value['capabilities'].get('point_probe_v1') is not True or artifact['filename']!=EXE):
            raise ValueError('Provider manifest contract mismatch')
        if not exe.is_file() or exe.is_symlink() or artifact['size']<=0 or exe.stat().st_size!=artifact['size']:
            raise ValueError('Provider size mismatch')
        validate_pe_x64(exe)
        if digest(exe)!=artifact['sha256']: raise ValueError('Provider hash mismatch')
        return value
    except (KeyError, TypeError, OSError, json.JSONDecodeError) as error:
        raise ValueError('Invalid Provider asset manifest') from error

def prepare(source, destination):
    source, destination=Path(source).resolve(),Path(destination).resolve()
    if not (source/'Cargo.toml').is_file(): raise ValueError(f'Provider source missing: {source}')
    with locked(source/'target/.uipilot-provider-build.lock'):
        print('UIPILOT_PROVIDER_BUILD', flush=True)
        target=source/'target/uipilot-provider'
        build_args=[*BUILD_ARGS,'--target-dir',str(target)]
        run(build_args, cwd=source)
        built=target/TARGET/'release/uiautoma-uiautomation-provider.exe'
        if not built.is_file(): raise ValueError('Provider build produced no executable')
        validate_pe_x64(built)
        caps=ping(built)
        if caps.get('point_probe_v1') is not True: raise ValueError('Provider lacks point_probe_v1')
        value={'schema_version':1,'generation':uuid.uuid4().hex,'source':{'commit':run(['git','rev-parse','HEAD'],cwd=source,capture=True).strip(),
               'dirty':bool(run(['git','status','--porcelain'],cwd=source,capture=True).strip())},
               'target':TARGET,'build_args':build_args,'capabilities':caps,
               'artifact':{'filename':EXE,'size':built.stat().st_size,'sha256':digest(built)}}
        destination.parent.mkdir(parents=True,exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='.provider-',dir=destination.parent) as tmp:
            stage=Path(tmp); shutil.copyfile(built,stage/EXE)
            (stage/MANIFEST).write_text(json.dumps(value,indent=2),encoding='utf-8'); validate(stage)
            with locked(destination.parent/'.provider-stage.lock'):
                destination.mkdir(parents=True,exist_ok=True)
                # Manifest is the commit marker. A crash between replacements fails validation.
                os.replace(stage/EXE,destination/EXE); os.replace(stage/MANIFEST,destination/MANIFEST)
                validate(destination)
        print('UIPILOT_PROVIDER_READY '+str(destination/EXE),flush=True)
        return destination/EXE

def main():
    parser=argparse.ArgumentParser(); parser.add_argument('action',choices=['prepare','validate','validate-dev','build-session'])
    parser.add_argument('--rust-uia',type=Path,default=None); args,extra=parser.parse_known_args()
    if args.action=='build-session':
        return build_session(ROOT,extra[1:] if extra[:1]==['--'] else extra)
    if extra: parser.error('unexpected arguments')
    destination=ROOT/'src-tauri/resources/provider'
    if args.action=='prepare':
        receipt,session=session_receipt()
        prepare(args.rust_uia or source_root(),destination)
        session['generation']=validate(destination)['generation']
        receipt.write_text(json.dumps(session),encoding='utf-8')
    elif args.action=='validate-dev':
        exe=Path(os.environ['UIPILOT_SCREENSHOT_PROVIDER_EXE'])
        if not exe.is_absolute() or exe.name!=EXE: raise ValueError('Invalid dev Provider path')
        manifest=validate(exe.parent)
        if manifest.get('generation')!=os.environ.get('UIPILOT_PROVIDER_GENERATION'):
            raise ValueError('Dev Provider generation mismatch')
    else:
        _,session=session_receipt()
        with locked(destination.parent/'.provider-stage.lock'):
            if validate(destination)['generation']!=session.get('generation'):
                raise ValueError('Production Provider generation mismatch')
        print('UIPILOT_PROVIDER_VALIDATED',flush=True)

if __name__=='__main__': raise SystemExit(main())
