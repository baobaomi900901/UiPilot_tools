"""Exercise real inherited Windows console handles without opening a visible window."""
import json
import os
from pathlib import Path
import subprocess
import shutil
import sys
import tempfile
import unittest


@unittest.skipUnless(os.name == 'nt', 'Windows console inheritance')
class DevConsoleTests(unittest.TestCase):
    def test_node_logs_reach_interactive_console(self):
        node = shutil.which('node.exe')
        self.assertIsNotNone(node)
        parent = r'''
import ctypes as c, json, sys, time
from ctypes import wintypes as w
from pathlib import Path
sys.path.insert(0,sys.argv[1])
from package_job import OwnedWorker
with OwnedWorker([sys.argv[3],'-e',"console.log('DEV_CONSOLE_OUT'); console.error('DEV_CONSOLE_ERR')"],sys.argv[1],inherit_stdio=True) as worker:
    deadline=time.monotonic()+10
    while worker.poll() is None and time.monotonic()<deadline: time.sleep(.02)
    code=worker.poll()
    if code is None: raise RuntimeError('node console fixture timed out')
k=c.WinDLL('kernel32',use_last_error=True)
k.GetStdHandle.argtypes=[w.DWORD]; k.GetStdHandle.restype=w.HANDLE
k.ReadConsoleOutputCharacterW.argtypes=[w.HANDLE,w.LPWSTR,w.DWORD,w.DWORD,c.POINTER(w.DWORD)]
text=c.create_unicode_buffer(4096); count=w.DWORD()
if not k.ReadConsoleOutputCharacterW(k.GetStdHandle(-11),text,4096,0,c.byref(count)): raise c.WinError(c.get_last_error())
Path(sys.argv[2]).write_text(json.dumps({'code':code,'text':text.value}))
'''
        with tempfile.TemporaryDirectory() as temporary:
            result_path = Path(temporary) / 'console.json'
            startup = subprocess.STARTUPINFO()
            startup.dwFlags = subprocess.STARTF_USESHOWWINDOW
            startup.wShowWindow = subprocess.SW_HIDE
            completed = subprocess.run(
                [sys.executable, '-c', parent, str(Path(__file__).parent), str(result_path), node],
                creationflags=subprocess.CREATE_NEW_CONSOLE, startupinfo=startup, timeout=20,
            )
            self.assertEqual(completed.returncode, 0)
            result = json.loads(result_path.read_text())
            self.assertEqual(result['code'], 0)
            self.assertIn('DEV_CONSOLE_OUT', result['text'])
            self.assertIn('DEV_CONSOLE_ERR', result['text'])

    def test_owned_worker_keeps_interactive_console_streams(self):
        child = r'''
import ctypes as c, json, sys
from ctypes import wintypes as w
from pathlib import Path
k=c.WinDLL('kernel32',use_last_error=True)
k.GetStdHandle.argtypes=[w.DWORD]; k.GetStdHandle.restype=w.HANDLE
k.GetConsoleMode.argtypes=[w.HANDLE,c.POINTER(w.DWORD)]
valid=[]
for value in (-10,-11,-12):
    mode=w.DWORD()
    valid.append(bool(k.GetConsoleMode(k.GetStdHandle(value),c.byref(mode))))
Path(sys.argv[1]).write_text(json.dumps(valid))
'''
        parent = r'''
import sys, time
sys.path.insert(0,sys.argv[1])
from package_job import OwnedWorker
with OwnedWorker([sys.executable,'-c',sys.argv[3],sys.argv[2]],sys.argv[1],inherit_stdio=True) as worker:
    deadline=time.monotonic()+10
    while worker.poll() is None and time.monotonic()<deadline: time.sleep(.02)
    if worker.poll() is None: raise RuntimeError('console fixture timed out')
    raise SystemExit(worker.poll())
'''
        with tempfile.TemporaryDirectory() as temporary:
            result_path = Path(temporary) / 'streams.json'
            startup = subprocess.STARTUPINFO()
            startup.dwFlags = subprocess.STARTF_USESHOWWINDOW
            startup.wShowWindow = subprocess.SW_HIDE
            completed = subprocess.run(
                [sys.executable, '-c', parent, str(Path(__file__).parent), str(result_path), child],
                creationflags=subprocess.CREATE_NEW_CONSOLE, startupinfo=startup, timeout=20,
            )
            self.assertEqual(completed.returncode, 0)
            self.assertEqual(json.loads(result_path.read_text()), [True, True, True],
                             'interactive stdin/stdout/stderr must remain attached to the console')


if __name__ == '__main__':
    unittest.main()
