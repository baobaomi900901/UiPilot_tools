import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import dev_uipilot_launcher as launcher


class LauncherTests(unittest.TestCase):
    def test_explicit_root_is_remembered_and_reused_with_arguments(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            root = directory / 'host with spaces'
            (root / 'scripts').mkdir(parents=True)
            (root / 'scripts/dev_uipilot.py').write_text('')
            with patch.object(launcher, '__file__', str(directory / 'dev-uipilot.py')):
                with patch('sys.argv', ['dev-uipilot.py', '--root', str(root), '--rust-uia', 'source with spaces']), patch.object(launcher.subprocess, 'call', return_value=0) as call:
                    self.assertEqual(launcher.main(), 0)
                    self.assertEqual(call.call_args.args[0][-2:], ['--rust-uia', 'source with spaces'])
                self.assertTrue((directory / 'dev-uipilot.config.json').is_file(), 'explicit root must be remembered')
                settings = json.loads((directory / 'dev-uipilot.config.json').read_text())
                self.assertEqual(settings['root'], str(root.resolve()))
                with patch('sys.argv', ['dev-uipilot.py', '--help']), patch.object(launcher.subprocess, 'call', return_value=0) as call:
                    launcher.main()
                    self.assertEqual(call.call_args.args[0][1], str(root / 'scripts/dev_uipilot.py'))


if __name__ == '__main__':
    unittest.main()
