"""Execute only fixed embedded helpers through the existing bounded supervisor."""
import json
import os
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / 'macos-jsc-runtime-probe'))
from supervisor import run

class Modules(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.root = Path(os.environ['UIPILOT_ESM_OUTPUT'])
        cls.graphs = json.loads((cls.root / 'modules.json').read_text())

    def checked(self, name, status='ok', **kwargs):
        outcome = run(self.root / name / 'helper', 'success', **kwargs)
        self.assertEqual(outcome.status, status, name)
        self.assertTrue(outcome.reaped)
        with self.assertRaises(ChildProcessError):
            os.waitpid(outcome.pid, os.WNOHANG)
        print(f'{name}: {outcome.status}, exact child {outcome.pid} reaped', flush=True)
        return outcome

    def test_real_demo_return_complete_chinese_emoji_output(self):
        text = '中文 😀  保留空格 2026-11-08'
        expected = {
            'requestId': 'request-1',
            'results': [{'id': 'demo-return-copy', 'title': text, 'subtitle': 'Demo return result',
                         'defaultAction': {'type': 'copyText', 'text': text}}],
        }
        self.assertEqual(self.checked('demo-return').payload, expected)

    def test_relative_reexport_live_binding_async_and_frozen_inputs(self):
        self.assertEqual(self.checked('relative').payload, {
            'requestId': 'request-1', 'data': {'before': 1, 'after': 2, 'frozen': True, 'retainedPureCall': True,
                                             'input': '中文 😀  保留空格'},
        })

    def test_esm_strict_semantics_survive_iife_conversion(self):
        self.assertEqual(self.checked('strict').payload, {
            'requestId': 'request-1', 'data': {'unboundThis': True, 'undeclaredThrows': True,
                                             'noGlobal': True, 'frozenWriteThrows': True},
        })

    def test_completion_channel_not_global_and_forgery_not_accepted(self):
        self.assertEqual(self.checked('private-channel').payload, {
            'requestId': 'request-1', 'data': {'finishType': 'undefined', 'runnerType': 'undefined', 'forged': False},
        })

    def test_captured_intrinsics_survive_module_global_mutation(self):
        self.assertEqual(self.checked('intrinsics').payload, {
            'requestId': 'request-1', 'data': 'captured intrinsics',
        })

    def test_module_response_id_independently_checked_and_failure_redacted(self):
        for name in ['wrong-request', 'throw', 'oversized']:
            with self.subTest(name=name):
                result = self.checked(name, 'scriptFailed')
                self.assertIsNone(result.payload)
                self.assertNotIn('PRIVATE', repr(result))
        self.assertEqual(self.checked('demo-return', generation='9', request_id='another').payload['requestId'], 'another')

    def test_translate_graph_bundles_but_bare_jsc_load_fails(self):
        graph = next(case for case in self.graphs['cases'] if case['name'] == 'translate')
        self.assertEqual({row['name'] for row in graph['inputs']}, {
            'dist/runtime.js', 'dist/languages.js', 'dist/providers.js',
        })
        self.assertEqual(self.checked('translate').payload, {
            'phase': 'moduleEvaluation', 'completed': False, 'textEncoderMissingBeforeLoad': True,
        })

    def test_real_top_level_loop_and_pending_promise_killed_reaped_and_recovery(self):
        for name in ['infinite', 'pending']:
            with self.subTest(name=name):
                old = self.checked(name, 'timedOut', timeout=0.3)
                fresh = self.checked('demo-return', generation='2', request_id='recovery')
                self.assertNotEqual(old.pid, fresh.pid)

if __name__ == '__main__': unittest.main()
