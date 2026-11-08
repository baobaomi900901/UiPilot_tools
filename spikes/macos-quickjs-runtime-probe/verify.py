import os
import json
from pathlib import Path
import unittest
from supervisor import run

class Native(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.helper = Path(os.environ['UIPILOT_QUICKJS_HELPER'])
        cls.resources = []

    @classmethod
    def tearDownClass(cls):
        if os.environ.get('UIPILOT_QUICKJS_RECORDS'):
            Path(os.environ['UIPILOT_QUICKJS_RECORDS']).write_text(json.dumps(cls.resources, indent=2) + '\n')

    def checked(self, case, status='ok', **kwargs):
        outcome = run(self.helper, case, **kwargs)
        self.assertEqual(outcome.status, status, case)
        self.assertTrue(outcome.reaped)
        with self.assertRaises(ChildProcessError): os.waitpid(outcome.pid, os.WNOHANG)
        if case in ['heapLow', 'heapHigh', 'stackLow', 'stackHigh', 'interrupt']:
            self.resources.append({'case': case, 'pid': outcome.pid, 'reaped': outcome.reaped, 'report': outcome.payload})
        print(json.dumps({'case': case, 'status': outcome.status, 'pid': outcome.pid,
                          'reaped': outcome.reaped, 'report': outcome.payload}, ensure_ascii=False), flush=True)
        return outcome

    def response(self, case):
        report = self.checked(case).payload
        self.assertEqual((report['phase'], report['outcome']), ('command', 'completed'))
        self.assertFalse(report['interruptFired'])
        return report['response']

    def recovered(self):
        self.assertEqual(self.response('demo')['requestId'], 'request-1')

    def test_real_unmodified_demo_return(self):
        text = '中文 😀  保留空格 2026-11-08'
        self.assertEqual(self.response('demo'), {
            'requestId': 'request-1', 'results': [{'id': 'demo-return-copy', 'title': text,
                'subtitle': 'Demo return result', 'defaultAction': {'type': 'copyText', 'text': text}}],
        })

    def test_raw_modules_cycle_live_bindings_dynamic_import_and_tla(self):
        self.assertEqual(self.response('modules'), {'requestId': 'request-1', 'data': {
            'before': [1, 10], 'after': [2, 12], 'live': 2, 'dynamicLive': 2, 'ready': True, 'frozen': True,
        }})

    def test_resolver_and_loader_refuse_external_modules_attributes_and_bytecode(self):
        self.assertEqual(self.response('dynamicDenied')['data'], [True] * 14)
        for case in ['staticStd', 'staticOs', 'staticEscape', 'attributes']:
            with self.subTest(case=case):
                self.checked(case, 'scriptFailed')
                self.recovered()

    def test_no_io_media_globals_and_no_public_completion_channel(self):
        values = self.response('globals')['data']
        self.assertEqual(len(values), 19)
        self.assertEqual(set(values.values()), {'undefined'})
        self.assertEqual(self.response('forged')['data'], {
            'finishType': 'undefined', 'runnerType': 'undefined', 'forged': False,
        })

    def test_raw_esm_strict_and_captured_intrinsics(self):
        self.assertEqual(self.response('strict')['data'], {
            'unboundThis': True, 'undeclaredThrows': True, 'noGlobal': True, 'frozenWriteThrows': True,
        })
        self.assertEqual(self.response('intrinsics')['data'], 'captured intrinsics')

    def test_bad_response_or_exception_is_redacted_and_new_helper_recovers(self):
        for case in ['wrongRequest', 'oversized', 'throw']:
            with self.subTest(case=case):
                failure = self.checked(case, 'scriptFailed')
                self.assertIsNone(failure.payload)
                self.assertNotIn('PRIVATE', repr(failure))
                self.recovered()
        result = self.checked('demo', generation='7', request_id='new-request').payload
        self.assertEqual(result['response']['requestId'], 'new-request')

    def test_real_engine_interrupt_returns_normally_and_recovers(self):
        report = self.checked('interrupt').payload
        self.assertEqual((report['phase'], report['outcome']), ('command', 'interrupted'))
        self.assertTrue(report['interruptFired'])
        self.recovered()

    def test_heap_limit_same_real_allocation_low_fails_high_passes(self):
        low = self.checked('heapLow').payload
        high = self.checked('heapHigh').payload
        self.assertEqual((low['phase'], low['outcome']), ('command', 'failed'))
        self.assertEqual((high['phase'], high['outcome']), ('command', 'completed'))
        self.assertEqual(high['response']['data'], 4 * 1024 * 1024)
        self.assertLess(low['limits']['heap'], high['limits']['heap'])
        for report in [low, high]:
            self.assertFalse(report['interruptFired'])
            self.assertEqual(report['memory']['mallocLimit'], report['limits']['heap'])
            self.assertLess(report['memory']['mallocSize'], report['limits']['heap'])
        self.assertGreater(high['memory']['memoryUsedSize'], 4 * 1024 * 1024)
        self.recovered()

    def test_stack_limit_same_recursion_low_fails_high_passes(self):
        low = self.checked('stackLow').payload
        high = self.checked('stackHigh').payload
        self.assertEqual((low['phase'], low['outcome']), ('command', 'failed'))
        self.assertEqual((high['phase'], high['outcome']), ('command', 'completed'))
        self.assertEqual(high['response']['data'], 100)
        self.assertLess(low['limits']['stack'], high['limits']['stack'])
        self.assertFalse(low['interruptFired'])
        self.recovered()

    def test_parent_fallback_kills_and_reaps_infinite_and_both_pending_phases(self):
        for case in ['parentInfinite', 'pending', 'modulePending']:
            with self.subTest(case=case):
                previous = self.checked(case, 'timedOut', timeout=0.3)
                fresh = self.checked('demo')
                self.assertNotEqual(previous.pid, fresh.pid)

    def test_invalid_protocol_input_rejected_before_spawn(self):
        for args in [('unknown', {}), ('demo', {'generation': True}), ('demo', {'request_id': ''})]:
            self.assertEqual(run(Path('/not-invoked'), args[0], **args[1]).status, 'invalidInput')

if __name__ == '__main__': unittest.main()
