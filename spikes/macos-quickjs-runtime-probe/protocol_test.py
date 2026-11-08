"""Synthetic child transport faults; engine behavior is separately tested in verify.py."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
from supervisor import run

GOOD = {'generation': '1', 'requestId': 'request-1', 'status': 'ok', 'value': None}

class Protocol(unittest.TestCase):
    def child(self, body, expected):
        with tempfile.TemporaryDirectory(prefix='uipilot-quickjs-wire-') as directory:
            helper = Path(directory) / 'fixed-child'
            helper.write_text(f'#!{sys.executable}\nimport sys, time\nsys.stdin.buffer.read()\n{body}\n')
            helper.chmod(0o700)
            result = run(helper, 'demo', timeout=0.3)
            self.assertEqual(result.status, expected)
            self.assertTrue(result.reaped)
            with self.assertRaises(ChildProcessError): os.waitpid(result.pid, os.WNOHANG)
            return result

    def test_duplicate_extra_malformed_and_nonfinite_responses(self):
        good = json.dumps(GOOD)
        wires = [good + good, '{', good.replace('"generation": "1"', '"generation":"1","generation":"1"'),
                 json.dumps({**GOOD, 'extra': True}), good.replace('null', '1e999')]
        for wire in wires:
            with self.subTest(wire=wire):
                self.child(f'sys.stdout.write({wire!r})', 'invalidResponse')

    def test_stale_and_bool_identity_are_not_current(self):
        for record in [{**GOOD, 'generation': '2'}, {**GOOD, 'generation': True}, {**GOOD, 'requestId': 'old'}]:
            self.child(f'sys.stdout.write({json.dumps(record)!r})', 'staleResponse')

    def test_combined_output_budget_counts_stderr_and_stdout(self):
        self.child("sys.stdout.write('x'*5000);sys.stdout.flush();sys.stderr.write('y'*5000)", 'outputTooLarge')

    def test_complete_response_before_spin_is_not_success(self):
        self.child(f'sys.stdout.write({json.dumps(GOOD)!r});sys.stdout.flush()\nwhile True: pass', 'timedOut')

    def test_continuous_stderr_cannot_starve_deadline_or_budget(self):
        self.child("while True: sys.stderr.write('x'*1024);sys.stderr.flush()", 'outputTooLarge')

    def test_parent_read_failure_kills_and_reaps_real_engine(self):
        with patch('supervisor.selectors.DefaultSelector.select', side_effect=OSError('PRIVATE_FAILURE')):
            result = run(Path(os.environ['UIPILOT_QUICKJS_HELPER']), 'pending')
        self.assertEqual(result.status, 'transportFailed')
        self.assertTrue(result.reaped)
        with self.assertRaises(ChildProcessError): os.waitpid(result.pid, os.WNOHANG)
        self.assertNotIn('PRIVATE', repr(result))

if __name__ == '__main__': unittest.main()
