import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch
import dev_uipilot as d
class DevTests(unittest.TestCase):
    def test_only_current_validated_asset_is_used_and_no_recursion(self):
        root=Path('D:/project space').resolve(); exe=root/'fresh/provider.exe'; calls=[]
        with patch.object(d.provider,'prepare',return_value=exe) as prepare, patch.object(d.provider,'validate',return_value={'generation':'fresh-generation'}) as validate, patch.object(d.shutil,'which',return_value='npm.cmd'):
            def runner(args,**kw):
                calls.append((args,kw)); return type('Result',(),{'returncode':7})()
            self.assertEqual(d.run_dev(root,Path('source'),['--no-watch'],runner),7)
        self.assertEqual(calls[0][0],['npm.cmd','run','tauri','--','dev','--no-watch'])
        self.assertEqual(calls[0][1]['env']['UIPILOT_SCREENSHOT_PROVIDER_EXE'],str(exe))
        validate.assert_called_once(); prepare.assert_called_once()
    def test_failed_prepare_never_launches(self):
        with patch.object(d.provider,'prepare',side_effect=ValueError('build failed')):
            runner=unittest.mock.Mock()
            with self.assertRaises(ValueError): d.run_dev(Path('.'),Path('.'),runner=runner)
            runner.assert_not_called()
