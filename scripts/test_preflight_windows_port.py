"""Portable fixture checks; the real Windows tools must still run on Windows."""
import contextlib
import io
from pathlib import Path
import tempfile
import unittest
from unittest import mock
import zipfile

import preflight_windows_port as preflight


class PreflightTests(unittest.TestCase):
    def test_archive_identity_covers_extracted_bytes_extra_files_and_archive_hash(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "source"
            root.mkdir()
            (root / "file").write_bytes(b"one")
            archive = root.parent / "candidate.zip"
            with zipfile.ZipFile(archive, "w") as bundle:
                bundle.writestr("UiPilot_tools/file", b"one")
                bundle.comment = preflight.CANDIDATE.encode("ascii")
            with mock.patch.object(preflight, "SOURCE_ZIP_SHA256", preflight.sha256(archive)):
                self.assertTrue(preflight.matches_archive(root, archive))
                (root / ".uipilot-local.json").write_text("{}")
                self.assertTrue(preflight.matches_archive(root, archive))
                (root / "extra").write_text("surprise")
                self.assertFalse(preflight.matches_archive(root, archive))
                (root / "extra").unlink()
                (root / "file").write_bytes(b"two")
                self.assertFalse(preflight.matches_archive(root, archive))
                (root / "file").write_bytes(b"one")
                with archive.open("ab") as stream:
                    stream.write(b"changed ZIP")
                self.assertFalse(preflight.matches_archive(root, archive))

    def test_rustup_proxy_never_downloads_an_absent_pinned_toolchain(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            with mock.patch.object(preflight.shutil, "which", side_effect=lambda exe: str(base / "bin" / exe)), \
                    mock.patch.dict(preflight.os.environ, RUSTUP_HOME=str(base)):
                self.assertIsNone(preflight.rust_binary("rustc", "1.96.1"))
                compiler = base / "toolchains" / f"1.96.1-{preflight.HOST}" / "bin" / "rustc.exe"
                compiler.parent.mkdir(parents=True)
                compiler.touch()
                self.assertEqual(preflight.rust_binary("rustc", "1.96.1"), str(compiler))

    def test_unavailable_provider_does_not_echo_private_external_path(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            private = root.parent / "device-private-directory-name"
            report = preflight.Report()
            output = io.StringIO()
            with mock.patch.dict(preflight.os.environ, UIPILOT_RUST_UIA_ROOT=str(private)), contextlib.redirect_stdout(output):
                preflight.check_provider(root, report, None)
            self.assertEqual(report.failures, 1)
            self.assertNotIn(str(private), output.getvalue())
            self.assertIn("BLOCKED Provider", output.getvalue())

    def test_pinned_native_tool_fixture_passes_without_compiling_or_downloading(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "1.96.1"')
            cli = root / "node_modules/npm/bin/npm-cli.js"
            cli.parent.mkdir(parents=True)
            cli.touch()

            def probe(args, _cwd):
                return {
                    "node.exe": "x64 22.22.1",
                    "rustc.exe": "release: 1.96.1\nhost: x86_64-pc-windows-msvc",
                    "cargo.exe": "cargo 1.96.1 (fixture)",
                    "powershell.exe": "5.1.19041.1",
                }[Path(args[0]).name]

            report = preflight.Report()
            with mock.patch.dict(preflight.os.environ, VSCMD_ARG_TGT_ARCH="x64", RUSTUP_TOOLCHAIN=""), \
                    mock.patch.object(preflight.shutil, "which", side_effect=lambda name: str(root / name)), \
                    mock.patch.object(preflight, "rust_binary", side_effect=lambda name, pin: str(root / (name + ".exe"))), \
                    mock.patch.object(preflight, "command", side_effect=probe), contextlib.redirect_stdout(io.StringIO()):
                preflight.check_tools(root, report)
            self.assertEqual(report.failures, 0)

    def test_wrong_tool_versions_report_blockers_without_raw_probe_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "1.96.1"')
            report = preflight.Report()
            output = io.StringIO()
            with mock.patch.object(preflight.shutil, "which", return_value=None), \
                    mock.patch.object(preflight, "rust_binary", return_value=str(root / "rustc")), \
                    mock.patch.object(preflight, "command", return_value="private compiler diagnostics"), \
                    contextlib.redirect_stdout(output):
                preflight.check_tools(root, report)
            self.assertGreater(report.failures, 0)
            self.assertIn("BLOCKED Rust", output.getvalue())
            self.assertIn("BLOCKED MSVC x64", output.getvalue())
            self.assertNotIn("private compiler diagnostics", output.getvalue())


if __name__ == "__main__":
    unittest.main()
