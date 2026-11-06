import tempfile
import json
from pathlib import Path
import unittest
from unittest.mock import Mock, patch
import provider_build as p
import struct

ORIGINAL_CAPABILITIES = {
    'win32_elementtree': True,
    'win32_inspect_tree_v2': True,
    'win32_observation_sessions': True,
}

def pe_bytes(machine=0x8664):
    data=bytearray(128); data[:2]=b'MZ'; struct.pack_into('<I',data,60,64)
    data[64:68]=b'PE\0\0'; struct.pack_into('<H',data,68,machine); struct.pack_into('<H',data,88,0x20b)
    return bytes(data)


class ProviderTests(unittest.TestCase):
    def test_failed_build_never_stages_old_exe(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); (root/'Cargo.toml').touch()
            old=root/'target/release/uiautoma-uiautomation-provider.exe'; old.parent.mkdir(parents=True); old.write_bytes(b'old')
            with patch.object(p,'run',side_effect=RuntimeError('cargo failed')):
                with self.assertRaises(RuntimeError): p.prepare(root,root/'stage')
            self.assertFalse((root/'stage'/p.EXE).exists())
    def test_build_order_dirty_spaces_and_hash(self):
        with tempfile.TemporaryDirectory(prefix='provider space ') as tmp:
            root=Path(tmp); (root/'Cargo.toml').touch(); calls=[]
            def run(args, **kw):
                calls.append(args)
                if args[0]=='cargo':
                    f=root/'target/uipilot-provider'/p.TARGET/'release/uiautoma-uiautomation-provider.exe'; f.parent.mkdir(parents=True); f.write_bytes(pe_bytes())
                    return ''
                return 'abc' if 'rev-parse' in args else ' M src/main.rs'
            with patch.object(p,'run',side_effect=run), patch.object(p,'ping',return_value=ORIGINAL_CAPABILITIES.copy()):
                exe=p.prepare(root,root/'stage')
            m=p.validate(exe.parent)
            self.assertEqual(m['capabilities'], ORIGINAL_CAPABILITIES)
            self.assertTrue(m['source']['dirty']); self.assertEqual(calls[0][:4],['cargo','build','--release','--locked'])
            exe.write_bytes(b'broken')
            with self.assertRaisesRegex(ValueError,'hash|size'): p.validate(exe.parent)
    def test_wrong_capability_build_never_replaces_previous_stage(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); (root/'Cargo.toml').touch()
            built=root/'target/uipilot-provider'/p.TARGET/'release/uiautoma-uiautomation-provider.exe'
            built.parent.mkdir(parents=True); built.write_bytes(pe_bytes())
            def run(args,**kw): return ''
            stage=root/'stage'; stage.mkdir(); (stage/p.EXE).write_bytes(b'previous')
            for missing in ORIGINAL_CAPABILITIES:
                with self.subTest(missing=missing):
                    caps={key: value for key,value in ORIGINAL_CAPABILITIES.items() if key!=missing}
                    caps['point_probe_v1']=True
                    with patch.object(p,'run',side_effect=run), patch.object(p,'ping',return_value=caps):
                        with self.assertRaisesRegex(ValueError,missing): p.prepare(root,stage)
                    self.assertEqual((stage/p.EXE).read_bytes(),b'previous')

    def test_ping_accepts_original_capabilities_without_point_probe(self):
        child=Mock(pid=123)
        reply={'id':1,'result':{'provider_pid':123,'capabilities':ORIGINAL_CAPABILITIES.copy()}}
        result=Mock(stdout=json.dumps(reply))
        with patch.object(p.subprocess,'Popen',return_value=child), patch.object(p.subprocess,'run',return_value=result):
            self.assertEqual(p.ping(Path('provider.exe')),ORIGINAL_CAPABILITIES)
        child.wait.assert_called_once_with(timeout=5)

    def test_ping_rejects_missing_or_disabled_original_capability(self):
        for capability in ORIGINAL_CAPABILITIES:
            for invalid in (None,False,'true'):
                with self.subTest(capability=capability,invalid=invalid):
                    caps=ORIGINAL_CAPABILITIES.copy()
                    if invalid is None: caps.pop(capability)
                    else: caps[capability]=invalid
                    caps['point_probe_v1']=True
                    child=Mock(pid=123)
                    reply={'id':1,'result':{'provider_pid':123,'capabilities':caps}}
                    result=Mock(stdout=json.dumps(reply))
                    with patch.object(p.subprocess,'Popen',return_value=child), patch.object(p.subprocess,'run',return_value=result):
                        with self.assertRaisesRegex(ValueError,capability): p.ping(Path('provider.exe'))
                    child.wait.assert_called_once_with(timeout=5)

    def test_manifest_rejects_missing_or_disabled_original_capability(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory=Path(tmp); exe=directory/p.EXE; exe.write_bytes(pe_bytes())
            manifest={'schema_version':1,'target':p.TARGET,'capabilities':ORIGINAL_CAPABILITIES.copy(),
                      'artifact':{'filename':p.EXE,'size':exe.stat().st_size,'sha256':p.digest(exe)}}
            (directory/p.MANIFEST).write_text(json.dumps(manifest))
            self.assertEqual(p.validate(directory)['capabilities'],ORIGINAL_CAPABILITIES)
            for capability in ORIGINAL_CAPABILITIES:
                for invalid in (None,False,'true'):
                    with self.subTest(capability=capability,invalid=invalid):
                        caps=ORIGINAL_CAPABILITIES.copy()
                        if invalid is None: caps.pop(capability)
                        else: caps[capability]=invalid
                        caps['point_probe_v1']=True
                        manifest['capabilities']=caps
                        (directory/p.MANIFEST).write_text(json.dumps(manifest))
                        with self.assertRaises(ValueError): p.validate(directory)

    def test_actual_pe_machine_rejects_x86(self):
        with tempfile.TemporaryDirectory() as tmp:
            exe=Path(tmp)/'fake.exe'; exe.write_bytes(pe_bytes(0x14c))
            with self.assertRaisesRegex(ValueError,'x64'): p.validate_pe_x64(exe)
            exe.write_bytes(pe_bytes()); p.validate_pe_x64(exe)

    def test_production_session_spans_runner_and_cleans_receipt(self):
        import os
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp)
            def runner(args,**kw):
                with patch.dict(os.environ,kw['env']):
                    receipt,value=p.session_receipt(root)
                    self.assertTrue(receipt.is_file()); self.assertNotIn('generation',value)
                    self.assertEqual(args[-1],'build')
                return type('Result',(),{'returncode':7})()
            self.assertEqual(p.build_session(root,['build'],runner),7)
            self.assertEqual(list((root/'artifacts/provider-build').glob('*.json')),[])

    def test_production_lock_blocks_another_process_until_release(self):
        import subprocess, sys, time
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); marker=root/'acquired'; lock=root/'session.lock'
            # Source string passed as argv, not a shell command.
            code="import sys;from pathlib import Path;sys.path.insert(0,sys.argv[1]);import provider_build as p;"+"\nwith p.locked(Path(sys.argv[2])):\n Path(sys.argv[3]).write_text('acquired')"
            with p.locked(lock):
                child=subprocess.Popen([sys.executable,'-c',code,str(p.ROOT/'scripts'),str(lock),str(marker)])
                time.sleep(.3)
                self.assertIsNone(child.poll()); self.assertFalse(marker.exists())
            self.assertEqual(child.wait(timeout=10),0); self.assertEqual(marker.read_text(),'acquired')

    def test_bundle_hooks_and_resources_use_relative_paths(self):
        import json
        config=json.loads((p.ROOT/'src-tauri/tauri.windows.conf.json').read_text(encoding='utf-8'))
        self.assertTrue(config['build']['beforeBuildCommand'].startswith('python scripts/provider_build.py prepare &&'))
        self.assertTrue(config['build']['beforeBundleCommand'].startswith('python scripts/provider_build.py validate &&'))
        for name in (p.EXE,p.MANIFEST): self.assertIn('resources/provider/'+name,config['bundle']['resources'])
        self.assertNotIn('D:',str(config))

    def test_wrong_capability_fails_closed(self):
        with tempfile.TemporaryDirectory() as tmp:
            d=Path(tmp); (d/p.EXE).write_bytes(b'x'); (d/p.MANIFEST).write_text('{}')
            with self.assertRaises(ValueError): p.validate(d)

if __name__=='__main__': unittest.main()
