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
                    self.assertEqual(call.call_args.args[0][-2:], ['--rust-uia', str(Path('source with spaces').resolve())])
                self.assertTrue((directory / 'dev-uipilot.config.json').is_file(), 'explicit root must be remembered')
                settings = json.loads((directory / 'dev-uipilot.config.json').read_text())
                self.assertEqual(settings['root'], str(root.resolve()))
                self.assertEqual(settings['rust_uia'], str(Path('source with spaces').resolve()))
                with patch('sys.argv', ['dev-uipilot.py', '--help']), patch.object(launcher.subprocess, 'call', return_value=0) as call:
                    launcher.main()
                    self.assertEqual(call.call_args.args[0][1], str(root / 'scripts/dev_uipilot.py'))


    def test_interactive_reuse_and_replace_both_paths(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            old = directory / 'old'; new = directory / 'new project'
            for root in (old, new):
                (root / 'scripts').mkdir(parents=True)
                (root / 'scripts/dev_uipilot.py').write_text('')
            config = directory / 'dev-uipilot.config.json'
            config.write_text(json.dumps({'root': str(old), 'rust_uia': str(directory/'old source')}))
            with patch.object(launcher, '__file__', str(directory/'dev-uipilot.py')), patch('sys.argv', ['dev-uipilot.py']), patch.object(launcher.sys.stdin, 'isatty', return_value=True), patch.object(launcher.subprocess, 'call', return_value=0) as call:
                with patch('builtins.input', return_value='') as answer:
                    self.assertEqual(launcher.main(), 0)
                    answer.assert_called_once()
                    self.assertIn(str(old), call.call_args.args[0])
                    self.assertIn(str(directory/'old source'), call.call_args.args[0])
                with patch('builtins.input', side_effect=['n', str(new), str(directory/'new source')]):
                    self.assertEqual(launcher.main(), 0)
                    self.assertIn(str(new), call.call_args.args[0])
                    self.assertIn(str(directory/'new source'), call.call_args.args[0])
                self.assertEqual(json.loads(config.read_text())['root'], str(new))

    def test_noninteractive_never_prompts_and_forwards_rebuild_flag(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            (directory/'scripts').mkdir()
            (directory/'scripts/dev_uipilot.py').write_text('')
            with patch.object(launcher, '__file__', str(directory/'dev-uipilot.py')), patch('sys.argv', ['dev-uipilot.py', '--root', str(directory), '--non-interactive', '--rebuild-provider']), patch('builtins.input', side_effect=AssertionError('no prompt')), patch.object(launcher.subprocess, 'call', return_value=0) as call:
                self.assertEqual(launcher.main(), 0)
                self.assertIn('--non-interactive', call.call_args.args[0])
                self.assertIn('--rebuild-provider', call.call_args.args[0])


if __name__ == '__main__':
    unittest.main()
