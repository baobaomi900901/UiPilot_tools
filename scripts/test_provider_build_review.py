import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
import provider_build as p
from test_provider_build import ORIGINAL_CAPABILITIES, pe_bytes

class BuildReviewTests(unittest.TestCase):
    def test_cargo_config_cannot_select_stale_default_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); (root/'Cargo.toml').touch()
            (root/'.cargo').mkdir(); (root/'.cargo/config.toml').write_text('[build]\ntarget-dir="elsewhere"')
            stale=root/'target'/p.TARGET/'release/uiautoma-uiautomation-provider.exe'
            stale.parent.mkdir(parents=True); stale.write_bytes(pe_bytes()+b'old')
            def run(args, **kw):
                if args[0]=='cargo':
                    self.assertIn('--target-dir', args)
                    target=Path(args[args.index('--target-dir')+1])
                    fresh=target/p.TARGET/'release'/stale.name
                    fresh.parent.mkdir(parents=True,exist_ok=True); fresh.write_bytes(pe_bytes()+b'current')
                return 'commit'
            with patch.object(p,'run',side_effect=run), patch.object(p,'ping',return_value=ORIGINAL_CAPABILITIES.copy()):
                exe=p.prepare(root,root/'stage')
            self.assertEqual(exe.read_bytes(),pe_bytes()+b'current')
            self.assertIn('--target-dir',p.validate(exe.parent)['build_args'])

    def test_bundle_prepares_once_before_cli_and_records_generation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); order=[]
            def prepare(*args): order.append('prepare')
            def runner(args,**kw):
                order.append('cli')
                with patch.dict(os.environ,kw['env']):
                    self.assertEqual(p.session_receipt(root)[1]['generation'],'new')
                return subprocess.CompletedProcess(args,0)
            with patch.object(p,'prepare',side_effect=prepare) as build, patch.object(p,'validate',return_value={'generation':'new'}):
                p.build_session(root,['bundle','--bundles','nsis'],runner)
            build.assert_called_once(); self.assertEqual(order,['prepare','cli'])

    @unittest.skipUnless(os.name=='nt','Windows job handles')
    def test_owned_cli_preserves_both_streams(self):
        code="import sys; sys.path.insert(0,sys.argv[1]); import provider_build as p; raise SystemExit(p.owned_cli([sys.executable,'-c',\"import sys; print('OWNED_OUT'); print('OWNED_ERR',file=sys.stderr)\"],cwd=sys.argv[1],env=dict(__import__('os').environ)).returncode)"
        result=subprocess.run([sys.executable,'-c',code,str(p.ROOT/'scripts')],capture_output=True,text=True,timeout=20)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertIn('OWNED_OUT',result.stdout); self.assertIn('OWNED_ERR',result.stderr)

    def test_explicit_provider_path_is_remembered(self):
        import dev_uipilot as d
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); source=root/'rust source'
            (root/'src-tauri').mkdir()
            (root/'src-tauri/tauri.conf.json').write_text('{}')
            with patch.object(d,'run_dev',return_value=0):
                self.assertEqual(d.main(['--root',str(root),'--rust-uia',str(source),'--non-interactive']), 0)
            self.assertEqual(__import__('json').loads((root/'.uipilot-local.json').read_text())['rust_uia'],str(source.resolve()))

    @unittest.skipUnless(os.name=='nt','Windows job handles')
    def test_stdio_worker_close_terminates_only_owned_tree(self):
        import ctypes
        from ctypes import wintypes
        import time
        from package_job import OwnedWorker
        with tempfile.TemporaryDirectory() as tmp:
            marker=Path(tmp)/'pid'
            code="import subprocess,sys,time; from pathlib import Path; child=subprocess.Popen([sys.executable,'-c','import time; time.sleep(60)']); Path(sys.argv[1]).write_text(str(child.pid)); time.sleep(60)"
            unrelated=subprocess.Popen([sys.executable,'-c','import time; time.sleep(60)'])
            api=ctypes.WinDLL('kernel32',use_last_error=True)
            api.OpenProcess.argtypes=[wintypes.DWORD,wintypes.BOOL,wintypes.DWORD]
            api.OpenProcess.restype=wintypes.HANDLE
            api.WaitForSingleObject.argtypes=[wintypes.HANDLE,wintypes.DWORD]
            api.CloseHandle.argtypes=[wintypes.HANDLE]
            handle=None
            try:
                with OwnedWorker([sys.executable,'-c',code,str(marker)],tmp,inherit_stdio=True):
                    deadline=time.monotonic()+10
                    while not marker.exists() and time.monotonic()<deadline: time.sleep(.05)
                    self.assertTrue(marker.exists())
                    handle=api.OpenProcess(0x100000,False,int(marker.read_text()))
                    self.assertTrue(handle)
                self.assertEqual(api.WaitForSingleObject(handle,5000),0)
                self.assertIsNone(unrelated.poll())
            finally:
                if handle: api.CloseHandle(handle)
                unrelated.terminate(); unrelated.wait(timeout=5)
