"""Focused packaging tests: no real Setup build, installer, or plugin execution."""
import hashlib
import json
from pathlib import Path
import tempfile
import os
import subprocess
import sys
import time
import unittest
from unittest.mock import patch

import package as pack


class PackagingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="uipilot-python-pack-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "中文 & project"
        (self.root / "src-tauri").mkdir(parents=True)
        pack.write_json(self.root / "src-tauri/tauri.conf.json",
                        {"version": "0.14.0", "productName": "UiPilot_tools"})
        self.source = self.root / "examples/public-plugins/a/package"
        self.source.mkdir(parents=True)
        pack.write_json(self.source / "plugin.json", {"pluginId": "a", "version": "1.0.0", "name": "测试"})
        self.tools = ("node.exe", "npm-cli.js")
        self.calls = []
        self.fail_at = 0

    def command(self, args, request):
        self.calls.append(args)
        if len(self.calls) == self.fail_at:
            return 7
        if "pack" in args:
            Path(args[args.index("--output") + 1]).write_bytes(b"new-package")
        if "nsis" in args:
            target = pack.setup_candidate(request)
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(b"new-setup")
        return 0

    def test_plugin_pipeline_preserves_literal_arguments_and_validates_archive(self):
        req = pack.new_run(self.root, "plugin", self.source)
        result = pack.build(req, self.tools, self.command)
        self.assertEqual(len(self.calls), 4)
        self.assertEqual(self.calls[1][3], str(self.source))
        self.assertEqual(self.calls[-1][3], result["artifact"])
        self.assertEqual(result["sha256"], hashlib.sha256(b"new-package").hexdigest())

    def test_fixed_output_overwrites_and_failure_preserves_previous(self):
        req = pack.new_run(self.root, "plugin", self.source)
        result = pack.build(req, self.tools, self.command)
        dest = pack.publish(req, result)
        self.assertEqual(dest, self.root / "artifacts/packages/plugins/a-1.0.0.uipilot-plugin")
        dest.write_bytes(b"previous")
        self.assertEqual(pack.publish(req, result).read_bytes(), b"new-package")
        dest.write_bytes(b"keep")
        Path(result["artifact"]).write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "hash"):
            pack.publish(req, result)
        self.assertEqual(dest.read_bytes(), b"keep")

    def test_failures_stop_each_stage(self):
        for stage in range(1, 5):
            with self.subTest(stage=stage):
                self.calls = []
                self.fail_at = stage
                req = pack.new_run(self.root, "plugin", self.source)
                with self.assertRaisesRegex(RuntimeError, "exit 7"):
                    pack.build(req, self.tools, self.command)
                self.assertEqual(len(self.calls), stage)

    def test_missing_archive_does_not_succeed(self):
        req = pack.new_run(self.root, "plugin", self.source)
        with self.assertRaises((ValueError, FileNotFoundError)):
            pack.build(req, self.tools, lambda *_: 0)

    def test_setup_fixed_command_and_stale_output(self):
        req = pack.new_run(self.root, "setup")
        result = pack.build(req, self.tools, self.command)
        self.assertEqual(self.calls[0], ["node.exe", "npm-cli.js", "run", "tauri", "--", "build", "--bundles", "nsis"])
        self.assertEqual(pack.publish(req, result), self.root / "artifacts/packages/setup/UiPilot_tools_0.14.0_x64-setup.exe")
        again = pack.new_run(self.root, "setup")
        with self.assertRaisesRegex(ValueError, "fresh"):
            pack.build(again, self.tools, lambda *_: 0)

    def test_invalid_metadata_and_nonordinary_output(self):
        for name in ("../escape", "CON", "a:b", "trailing."):
            pack.write_json(self.source / "plugin.json", {"pluginId": name, "version": "1.0.0"})
            with self.assertRaises(ValueError):
                pack.new_run(self.root, "plugin", self.source)
        output = self.root / "artifacts"
        output.write_text("not-directory")
        with self.assertRaises(ValueError):
            pack.new_run(self.root, "setup")

    def test_plugin_list_skips_nonobject_manifest(self):
        broken = self.source.parent.parent / "broken/package"
        broken.mkdir(parents=True)
        for value in (None, [], "invalid"):
            pack.write_json(broken / "plugin.json", value)
            with patch("builtins.print"):
                self.assertEqual([source for source, _ in pack.plugins(self.root)], [self.source])

    def test_cancel_and_mismatched_result_never_publish(self):
        req = pack.new_run(self.root, "plugin", self.source)
        result = pack.build(req, self.tools, self.command)
        bad = {**result, "runId": "wrong"}
        with self.assertRaises(ValueError):
            pack.publish(req, bad)
        (Path(req["runDirectory"]) / "cancel.flag").touch()
        with self.assertRaises(RuntimeError):
            pack.publish(req, result)

    def test_hash_vector_and_run_isolation(self):
        path = self.root / "abc"
        path.write_bytes(b"abc")
        self.assertEqual(pack.hash_file(path), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        self.assertNotEqual(pack.new_run(self.root, "setup")["runDirectory"], pack.new_run(self.root, "setup")["runDirectory"])

    def test_menu_selection_invalid_retry_and_eof(self):
        with patch("builtins.input", side_effect=["oops", "3", "2"]):
            self.assertEqual(pack.read_choice(2), 2)
        with patch("builtins.input", side_effect=EOFError):
            self.assertEqual(pack.read_choice(2), 0)
        with patch("builtins.input", side_effect=["2", "1"]):
            self.assertEqual(pack.choose_package(self.root), ("plugin", self.source))
        with patch("builtins.input", return_value="1"):
            self.assertEqual(pack.choose_package(self.root), ("setup", None))

    def test_child_environment_does_not_mutate_parent(self):
        with patch.dict("os.environ", {"CARGO_TARGET_DIR": "wrong", "TAURI_CONFIG": "wrong"}):
            import os
            env = pack.build_environment()
            self.assertNotIn("CARGO_TARGET_DIR", env)
            self.assertNotIn("TAURI_CONFIG", env)
            self.assertEqual(os.environ["CARGO_TARGET_DIR"], "wrong")

    @unittest.skipUnless(os.name == "nt", "Windows job ownership")
    def test_owned_worker_preserves_exit_code_and_literal_paths(self):
        from package_job import OwnedWorker
        output = self.root / "argv 中文 &.json"
        values = [str(output), 'quote"and\\', "$(not-a-command)"]
        code = "import json,sys;from pathlib import Path;Path(sys.argv[1]).write_text(json.dumps(sys.argv[1:]),encoding='utf-8');sys.exit(7)"
        with OwnedWorker([sys.executable, "-c", code, *values], self.root) as job:
            deadline = time.monotonic() + 10
            while job.poll() is None and time.monotonic() < deadline:
                time.sleep(0.02)
            self.assertEqual(job.poll(), 7)
        self.assertEqual(json.loads(output.read_text(encoding="utf-8")), values)

    @unittest.skipUnless(os.name == "nt", "Windows job ownership")
    def test_job_close_kills_descendant_but_not_unrelated_process(self):
        import ctypes as c
        from ctypes import wintypes as w
        from package_job import OwnedWorker, kernel_api
        api = kernel_api()
        api.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
        api.OpenProcess.restype = w.HANDLE
        pidfile = self.root / "child.pid"
        code = ("import subprocess,sys,time;from pathlib import Path;"
                "p=subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)'],creationflags=0x08000000);"
                "path=Path(sys.argv[1]);tmp=path.with_suffix('.tmp');"
                "tmp.write_text(str(p.pid));tmp.replace(path);time.sleep(30)")
        unrelated = subprocess.Popen([sys.executable, "-c", "import time;time.sleep(30)"], creationflags=subprocess.CREATE_NO_WINDOW)
        handle = None
        try:
            with OwnedWorker([sys.executable, "-c", code, str(pidfile)], self.root) as job:
                deadline = time.monotonic() + 10
                while not pidfile.exists() and time.monotonic() < deadline:
                    time.sleep(0.02)
                self.assertTrue(pidfile.exists(), "owned child started")
                handle = api.OpenProcess(0x100000, False, int(pidfile.read_text()))
                self.assertTrue(handle)
                self.assertIsNone(job.poll())
            self.assertEqual(api.WaitForSingleObject(handle, 5000), 0, "descendant exited")
            self.assertIsNone(unrelated.poll(), "unrelated process remains alive")
        finally:
            if handle:
                api.CloseHandle(handle)
            unrelated.terminate()
            unrelated.wait(timeout=10)


if __name__ == "__main__":
    unittest.main()
