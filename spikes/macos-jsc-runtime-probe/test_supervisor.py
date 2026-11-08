import unittest
from unittest.mock import patch
import subprocess
import os
import time
from pathlib import Path
from supervisor import run

class Admission(unittest.TestCase):
    def test_input_is_bounded_before_spawn(self):
        self.assertEqual(run(Path('/not-invoked'), 'success', request_id='x' * 5000).status, 'invalidInput')
    def test_case_and_identity_are_strict(self):
        for kwargs in [dict(case='../source'), dict(case='success', generation='0'), dict(case='success', generation='+1'), dict(case='success', request_id='')]:
            self.assertEqual(run(Path('/not-invoked'), **kwargs).status, 'invalidInput')

class NativeHelper(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.helper = Path(os.environ['UIPILOT_JSC_HELPER'])
    def checked(self, case, expected, **kwargs):
        result = run(self.helper, case, **kwargs)
        self.assertEqual(result.status, expected)
        self.assertTrue(result.reaped)
        self.assertIsNotNone(result.pid)
        with self.assertRaises(ChildProcessError): os.waitpid(result.pid, os.WNOHANG)
        return result
    def test_sync_async_and_no_media_or_native_globals(self):
        self.assertEqual(self.checked('success','ok').payload, {'text':'synthetic','sum':5})
        self.assertEqual(self.checked('async','ok').payload, {'text':'async synthetic'})
        globals = self.checked('globals','ok').payload
        self.assertEqual(len(globals), 16)
        self.assertEqual(set(globals.values()), {'undefined'})
    def test_esm_is_rejected_without_source_rewriting(self):
        self.checked('esm','esmUnsupported')
    def test_script_error_is_fixed_and_redacted(self):
        result = self.checked('throw','scriptFailed')
        self.assertIsNone(result.payload)
        self.assertNotIn('PRIVATE', repr(result))
    def test_bounded_protocol_rejects_bad_and_duplicate_completions(self):
        for case, status in [('malformed','invalidResponse'),('duplicate','invalidResponse'),
                             ('duplicateKeys','invalidResponse'),('extraField','invalidResponse'),
                             ('booleanOwner','staleResponse'),('stale','staleResponse'),
                             ('nonfinite','invalidResponse'),('wrongRequest','staleResponse'),
                             ('oversized','outputTooLarge'),('stderrFlood','outputTooLarge')]:
            with self.subTest(case=case): self.checked(case,status)
    def test_combined_stream_budget_rejects_two_individually_small_streams(self):
        self.checked('combinedFlood','outputTooLarge')
    def test_second_js_completion_is_a_script_failure_not_first_wins(self):
        result = self.checked('doubleFinish','scriptFailed')
        self.assertIsNone(result.payload)
    def test_parent_read_failure_also_kills_and_reaps_real_child(self):
        children = []
        spawn = subprocess.Popen
        def capture(*args, **kwargs):
            child = spawn(*args, **kwargs); children.append(child); return child
        with patch('supervisor.subprocess.Popen', side_effect=capture), patch('supervisor.selectors.DefaultSelector.select', side_effect=OSError('PRIVATE_PARENT_ERROR')):
            result = self.checked('infinite','transportFailed')
        self.assertEqual(len(children), 1)
        self.assertIsNotNone(children[0].returncode)
        self.assertNotIn('PRIVATE', repr(result))
    def test_real_deadline_kills_and_reaps_then_new_helper_recovers(self):
        for case in ['infinite','pending','finishThenSpin']:
            with self.subTest(case=case):
                start = time.monotonic()
                old = self.checked(case,'timedOut',timeout=0.3)
                self.assertLess(time.monotonic()-start,3)
                fresh = self.checked('success','ok',generation='2',request_id='new-request')
                self.assertNotEqual(old.pid, fresh.pid)

if __name__ == '__main__': unittest.main()

