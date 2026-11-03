import json
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch, Mock
import dev_uipilot as d


class DevTests(unittest.TestCase):
    def fixture(self, root):
        source = root / 'rust source'
        stage = root / 'artifacts/provider-dev' / ('a' * 32)
        stage.mkdir(parents=True)
        manifest = {'generation': 'generation-a', 'build_args': ['cargo', '--target-dir', str(source / 'target/uipilot-provider')]}
        (stage / d.provider.MANIFEST).write_text(json.dumps(manifest))
        return source, stage, manifest

    def test_reuse_migrates_existing_generation_and_never_builds_or_probes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source, stage, manifest = self.fixture(root)
            with patch.object(d.provider, 'validate', return_value=manifest), patch.object(d.provider, 'prepare') as prepare, patch.object(d.provider, 'ping') as ping:
                first = d.prepared_provider(root, source)
                second = d.prepared_provider(root, source)
            self.assertEqual(first, (stage / d.provider.EXE, manifest))
            self.assertEqual(first, second)
            prepare.assert_not_called(); ping.assert_not_called()
            self.assertTrue((stage.parent / 'current.json').is_file())

    def test_missing_or_different_source_never_silently_rebuilds(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            with patch.object(d.provider, 'prepare') as prepare:
                with self.assertRaisesRegex(ValueError, '首次运行'):
                    d.prepared_provider(root, root/'source')
                prepare.assert_not_called()
            source, stage, manifest = self.fixture(root)
            d.write_json(stage.parent / 'current.json', {'source': str(source), 'stage': stage.name, 'generation': 'generation-a'})
            with self.assertRaisesRegex(ValueError, '不同'):
                d.prepared_provider(root, root/'another')

    def test_invalid_cache_never_falls_back_to_older_generation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source, stage, manifest = self.fixture(root)
            d.write_json(stage.parent / 'current.json', {'source': str(source), 'stage': stage.name, 'generation': 'wrong'})
            with patch.object(d.provider, 'validate', return_value=manifest), patch.object(d.provider, 'prepare') as prepare:
                with self.assertRaisesRegex(ValueError, '批次不一致'):
                    d.prepared_provider(root, source)
                prepare.assert_not_called()

    def test_rebuild_publishes_only_after_success_and_preserves_previous_receipt_on_failure(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source, old_stage, manifest = self.fixture(root)
            receipt = old_stage.parent / 'current.json'
            original = {'source': str(source), 'stage': old_stage.name, 'generation': manifest['generation']}
            d.write_json(receipt, original)
            with patch.object(d.provider, 'prepare', side_effect=ValueError('build failed')):
                with self.assertRaisesRegex(ValueError, 'build failed'):
                    d.prepared_provider(root, source, True)
            self.assertEqual(json.loads(receipt.read_text()), original)
            with patch.object(d.provider, 'prepare', side_effect=lambda source, stage: stage / d.provider.EXE) as prepare, patch.object(d.provider, 'validate', return_value={'generation': 'fresh'}):
                exe, manifest = d.prepared_provider(root, source, True)
            prepare.assert_called_once()
            saved = json.loads(receipt.read_text())
            self.assertEqual(saved['stage'], exe.parent.name)
            self.assertEqual(saved['generation'], 'fresh')

    def test_reused_asset_still_runs_tauri_and_preserves_extra_arguments(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); exe = root/'cached'/d.provider.EXE
            runner = Mock(return_value=Mock(returncode=7))
            with patch.object(d, 'prepared_provider', return_value=(exe, {'generation': 'cached'})), patch.object(d.shutil, 'which', return_value='npm.cmd'):
                self.assertEqual(d.run_dev(root, root/'source', ['--no-watch'], runner), 7)
            self.assertEqual(runner.call_args.args[0], ['npm.cmd','run','tauri','--','dev','--no-watch'])
            env = runner.call_args.kwargs['env']
            self.assertEqual(env['UIPILOT_SCREENSHOT_PROVIDER_EXE'], str(exe))
            self.assertEqual(env['UIPILOT_PROVIDER_GENERATION'], 'cached')
            self.assertIn('artifacts', env['UIPILOT_DEV_STARTUP_LOG'])

    def test_failed_prepare_never_launches(self):
        with patch.object(d, 'prepared_provider', side_effect=ValueError('missing')):
            runner = Mock()
            with self.assertRaises(ValueError):
                d.run_dev(Path('.'),Path('.'),runner=runner)
            runner.assert_not_called()

    def test_prompt_default_reuses_and_yes_rebuilds(self):
        for answer, rebuild in [('', False), ('n', False), ('y', True)]:
            with tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp); (root/'src-tauri').mkdir(); (root/'src-tauri/tauri.conf.json').write_text('{}')
                with patch.object(d.sys.stdin,'isatty',return_value=True), patch('builtins.input',return_value=answer), patch.object(d,'run_dev',return_value=0) as run:
                    self.assertEqual(d.main(['--root',str(root)]),0)
                self.assertEqual(run.call_args.kwargs['rebuild'], rebuild)


if __name__ == '__main__': unittest.main()
