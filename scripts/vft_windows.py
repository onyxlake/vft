"""Windows-only process measurements and movement of a test-owned sender window."""

import ctypes
from ctypes import wintypes


class ProcessMetrics:
    def __init__(self, process):
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.psapi = ctypes.WinDLL("psapi", use_last_error=True)
        self.kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        self.kernel.OpenProcess.restype = wintypes.HANDLE
        self.kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        self.kernel.CloseHandle.restype = wintypes.BOOL
        self.kernel.GetProcessTimes.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4
        self.kernel.GetProcessTimes.restype = wintypes.BOOL

        class MemoryCounters(ctypes.Structure):
            _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD)] + [
                (name, ctypes.c_size_t) for name in (
                    "PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage",
                    "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage", "QuotaNonPagedPoolUsage",
                    "PagefileUsage", "PeakPagefileUsage",
                )
            ]

        self.counters = MemoryCounters
        self.psapi.GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(MemoryCounters), wintypes.DWORD]
        self.psapi.GetProcessMemoryInfo.restype = wintypes.BOOL
        self.handle = self.kernel.OpenProcess(0x0410, False, process.pid)
        # Very short transfers can exit before OpenProcess; their metrics are unknown.
        self.cpu = None
        self.peak = None

    def __enter__(self):
        self.sample()
        return self

    def sample(self):
        if not self.handle:
            return
        times = [wintypes.FILETIME() for _ in range(4)]
        if self.kernel.GetProcessTimes(self.handle, *[ctypes.byref(item) for item in times]):
            self.cpu = sum((item.dwHighDateTime << 32) | item.dwLowDateTime for item in times[2:]) / 10000000
        memory = self.counters()
        memory.cb = ctypes.sizeof(memory)
        if self.psapi.GetProcessMemoryInfo(self.handle, ctypes.byref(memory), memory.cb):
            self.peak = max(self.peak or 0, memory.PeakWorkingSetSize)

    def values(self, prefix):
        self.sample()
        return {f"{prefix}_cpu_seconds": self.cpu, f"{prefix}_peak_memory_bytes": self.peak}

    def __exit__(self, *_):
        if self.handle:
            self.kernel.CloseHandle(self.handle)


def move_sender(process):
    user = ctypes.WinDLL("user32", use_last_error=True)
    # SetWindowPos coordinates must be physical pixels, matching the native adapters.
    user.SetProcessDpiAwarenessContext.argtypes = [wintypes.HANDLE]
    user.SetProcessDpiAwarenessContext.restype = wintypes.BOOL
    user.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    user.EnumWindows.argtypes = [callback_type, wintypes.LPARAM]
    user.EnumWindows.restype = wintypes.BOOL
    user.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user.GetWindowThreadProcessId.restype = wintypes.DWORD
    user.IsWindowVisible.argtypes = [wintypes.HWND]
    user.IsWindowVisible.restype = wintypes.BOOL
    user.SetWindowPos.argtypes = [wintypes.HWND, wintypes.HWND, ctypes.c_int, ctypes.c_int,
                                  ctypes.c_int, ctypes.c_int, wintypes.UINT]
    user.SetWindowPos.restype = wintypes.BOOL
    windows = []

    @callback_type
    def visit(window, _):
        pid = wintypes.DWORD()
        user.GetWindowThreadProcessId(window, ctypes.byref(pid))
        if pid.value == process.pid and user.IsWindowVisible(window):
            windows.append(window)
        return True

    user.EnumWindows(visit, 0)
    if not windows or not user.SetWindowPos(windows[0], None, 180, 120, 0, 0, 0x0015):
        raise RuntimeError("Could not move the sender window owned by this test")
