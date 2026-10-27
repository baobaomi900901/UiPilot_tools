"""Windows worker ownership. A suspended child joins the job before it can run.

This is the ctypes equivalent of the former PowerShell/C# packaging helper.
Closing the job handle terminates only this worker and its descendants.
"""
import ctypes as c
from ctypes import wintypes as w
import subprocess


class StartupInfo(c.Structure):
    _fields_ = [("cb", w.DWORD), ("reserved", w.LPWSTR), ("desktop", w.LPWSTR),
                ("title", w.LPWSTR), ("x", w.DWORD), ("y", w.DWORD),
                ("xsize", w.DWORD), ("ysize", w.DWORD), ("xchars", w.DWORD),
                ("ychars", w.DWORD), ("fill", w.DWORD), ("flags", w.DWORD),
                ("show", w.WORD), ("reserved_size", w.WORD), ("reserved2", c.c_void_p),
                ("stdin", w.HANDLE), ("stdout", w.HANDLE), ("stderr", w.HANDLE)]


class ProcessInfo(c.Structure):
    _fields_ = [("process", w.HANDLE), ("thread", w.HANDLE),
                ("pid", w.DWORD), ("tid", w.DWORD)]


class BasicLimits(c.Structure):
    _fields_ = [("user", c.c_longlong), ("kernel", c.c_longlong), ("flags", w.DWORD),
                ("minimum", c.c_size_t), ("maximum", c.c_size_t), ("active", w.DWORD),
                ("affinity", c.c_size_t), ("priority", w.DWORD), ("scheduling", w.DWORD)]


class IoCounters(c.Structure):
    _fields_ = [(name, c.c_ulonglong) for name in ("read", "write", "other", "rb", "wb", "ob")]


class ExtendedLimits(c.Structure):
    _fields_ = [("basic", BasicLimits), ("io", IoCounters), ("process_memory", c.c_size_t),
                ("job_memory", c.c_size_t), ("peak_process", c.c_size_t), ("peak_job", c.c_size_t)]


def kernel_api():
    api = c.WinDLL("kernel32", use_last_error=True)
    signatures = {
        "CreateJobObjectW": ([c.c_void_p, w.LPCWSTR], w.HANDLE),
        "SetInformationJobObject": ([w.HANDLE, c.c_int, c.c_void_p, w.DWORD], w.BOOL),
        "AssignProcessToJobObject": ([w.HANDLE, w.HANDLE], w.BOOL),
        "CreateProcessW": ([w.LPCWSTR, w.LPWSTR, c.c_void_p, c.c_void_p, w.BOOL,
                            w.DWORD, c.c_void_p, w.LPCWSTR, c.POINTER(StartupInfo),
                            c.POINTER(ProcessInfo)], w.BOOL),
        "ResumeThread": ([w.HANDLE], w.DWORD),
        "WaitForSingleObject": ([w.HANDLE, w.DWORD], w.DWORD),
        "GetExitCodeProcess": ([w.HANDLE, c.POINTER(w.DWORD)], w.BOOL),
        "TerminateProcess": ([w.HANDLE, w.UINT], w.BOOL),
        "CloseHandle": ([w.HANDLE], w.BOOL),
    }
    for name, (args, result) in signatures.items():
        fn = getattr(api, name)
        fn.argtypes, fn.restype = args, result
    return api


def checked(value):
    if not value:
        raise c.WinError(c.get_last_error())
    return value


class OwnedWorker:
    def __init__(self, args: list[str], cwd):
        self.api = kernel_api()
        self.job = self.process = None
        info = ProcessInfo()
        try:
            self.job = checked(self.api.CreateJobObjectW(None, None))
            limits = ExtendedLimits()
            limits.basic.flags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            checked(self.api.SetInformationJobObject(self.job, 9, c.byref(limits), c.sizeof(limits)))
            startup = StartupInfo()
            startup.cb = c.sizeof(startup)
            startup.flags = 1  # STARTF_USESHOWWINDOW, SW_HIDE
            command = c.create_unicode_buffer(subprocess.list2cmdline(args))
            checked(self.api.CreateProcessW(args[0], command, None, None, False,
                                            0x08000004, None, str(cwd),
                                            c.byref(startup), c.byref(info)))
            self.process = info.process
            checked(self.api.AssignProcessToJobObject(self.job, self.process))
            if self.api.ResumeThread(info.thread) == 0xFFFFFFFF:
                raise c.WinError(c.get_last_error())
        except BaseException:
            if self.process:
                self.api.TerminateProcess(self.process, 1)
            self.close()
            raise
        finally:
            if info.thread:
                self.api.CloseHandle(info.thread)

    def poll(self):
        state = self.api.WaitForSingleObject(self.process, 0)
        if state == 258:  # WAIT_TIMEOUT, process still running
            return None
        if state != 0:
            raise c.WinError(c.get_last_error())
        code = w.DWORD()
        checked(self.api.GetExitCodeProcess(self.process, c.byref(code)))
        return code.value

    def close(self):
        if self.job:
            self.api.CloseHandle(self.job)
            self.job = None
        if self.process:
            self.api.CloseHandle(self.process)
            self.process = None

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()
