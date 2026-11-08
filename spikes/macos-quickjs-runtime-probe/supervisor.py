"""Fixed-fixture experiment: direct child process, bounded wire, kill AND reap.
Only this fixed experiment; no production sandbox or host authorization.
"""
from dataclasses import dataclass
from pathlib import Path
import json
import math
import os
import selectors
import subprocess
import time

INPUT_LIMIT = 4096
OUTPUT_LIMIT = 8192
CASES = frozenset((
    'demo', 'modules', 'dynamicDenied', 'staticStd', 'staticOs', 'staticEscape',
    'attributes', 'globals', 'strict', 'forged', 'intrinsics', 'wrongRequest',
    'oversized', 'throw', 'interrupt', 'parentInfinite', 'pending', 'modulePending',
    'heapLow', 'heapHigh', 'stackLow', 'stackHigh',
))

@dataclass(frozen=True)
class Outcome:
    status: str
    payload: object = None
    pid: int | None = None
    reaped: bool = False

def _unique(pairs):
    result = {}
    for key, value in pairs:
        if key in result: raise ValueError('duplicate key')
        result[key] = value
    return result

def _finite(text):
    number = float(text)
    if not math.isfinite(number): raise ValueError('nonfinite')
    return number

def run(helper: Path, case: str, generation: str = '1', request_id: str = 'request-1', timeout: float = 1.0) -> Outcome:
    if (not isinstance(case, str) or case not in CASES or not isinstance(generation, str) or not generation.isascii()
        or not generation.isdigit() or generation.startswith('0') or len(generation) > 20
        or int(generation) > 2**64 - 1 or not isinstance(request_id, str)
        or not 1 <= len(request_id) <= 128 or '\0' in request_id
        or isinstance(timeout, bool) or not isinstance(timeout, (int, float)) or not math.isfinite(timeout) or not 0 < timeout <= 10):
        return Outcome('invalidInput')
    wire = json.dumps(dict(case=case, generation=generation, requestId=request_id), ensure_ascii=True).encode()
    if len(wire) > INPUT_LIMIT: return Outcome('invalidInput')
    process = None
    output = bytearray()
    total = 0
    failure = None
    deadline = time.monotonic() + timeout
    try:
        # No shell, PATH lookup, inherited stdin or arbitrary package path. The
        # caller supplies the fixed helper built by this experiment's build.sh.
        process = subprocess.Popen([str(helper.resolve())], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   close_fds=True, env={'LANG': 'C', 'LC_ALL': 'C'})
        with selectors.DefaultSelector() as selector:
            for stream in (process.stdin, process.stdout, process.stderr):
                os.set_blocking(stream.fileno(), False)
            selector.register(process.stdin, selectors.EVENT_WRITE, 'input')
            selector.register(process.stdout, selectors.EVENT_READ, 'output')
            selector.register(process.stderr, selectors.EVENT_READ, 'error')
            sent = 0
            while selector.get_map():
                remaining = deadline - time.monotonic()
                if remaining <= 0: failure = 'timedOut'; break
                for key, _ in selector.select(min(remaining, 0.05)):
                    stream = key.fileobj
                    if key.data == 'input':
                        try: sent += os.write(stream.fileno(), wire[sent:])
                        except BrokenPipeError: sent = len(wire)
                        if sent == len(wire): selector.unregister(stream); stream.close()
                        continue
                    chunk = os.read(stream.fileno(), min(1024, OUTPUT_LIMIT + 1 - total))
                    if not chunk: selector.unregister(stream); stream.close(); continue
                    total += len(chunk)
                    if total > OUTPUT_LIMIT: failure = 'outputTooLarge'; break
                    if key.data == 'output': output.extend(chunk)
                if failure: break
            if not failure:
                try: process.wait(timeout=max(0.001, deadline-time.monotonic()))
                except subprocess.TimeoutExpired: failure = 'timedOut'
    except (OSError, ValueError):
        failure = 'transportFailed'
    finally:
        if process is not None:
            if process.poll() is None: process.kill()
            # Successful return ALWAYS means the exact child was reaped. We do
            # not return a fake timeout success while an interpreter still runs.
            process.wait()
            for stream in (process.stdin, process.stdout, process.stderr):
                if stream and not stream.closed: stream.close()
    if process is None: return Outcome(failure or 'transportFailed')
    def result(status, payload=None): return Outcome(status, payload, process.pid, True)
    if failure: return result(failure)
    if process.returncode != 0: return result('helperFailed')
    try:
        record = json.loads(output.decode('utf-8', errors='strict'), object_pairs_hook=_unique, parse_float=_finite,
                            parse_constant=lambda _: (_ for _ in ()).throw(ValueError()))
        if not isinstance(record, dict) or set(record) != {'generation', 'requestId', 'status', 'value'}:
            return result('invalidResponse')
        if record['generation'] != generation or record['requestId'] != request_id:
            return result('staleResponse')
        if record['status'] not in ('ok', 'scriptFailed', 'esmUnsupported'):
            return result('invalidResponse')
        if record['status'] != 'ok' and record['value'] is not None: return result('invalidResponse')
        return result(record['status'], record['value'])
    except (ValueError, UnicodeError, RecursionError, TypeError):
        return result('invalidResponse')
