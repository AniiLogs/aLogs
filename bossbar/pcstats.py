"""What the PC is doing, for the meter's once-a-minute "load:" line and a one-time "PC:" line - so a lag report shows
where the time went. Windows' own counters and a few kernel calls only (no installs, nothing sent anywhere else).
Never other programs' names: only the game, aLogs' own programs (meter, app, Sanctum Helper) and the totals.

- GpuLoad: graphics card use per program, as Task Manager shows it (its busiest engine), from the
  "GPU Engine(*)\\Utilization Percentage" counters; read once a minute (the counter averages since the last read).
- memory(): RAM free and total, and what aLogs' programs use (working set).
- system_line(): graphics card, CPU, RAM, monitor mode, and how the game's window sits (fullscreen / borderless /
  windowed), once when the game window is found.
"""
import ctypes, ctypes.wintypes as W, json, os, re, winreg

kernel32 = ctypes.windll.kernel32; user32 = ctypes.windll.user32
kernel32.OpenProcess.restype = ctypes.c_void_p
kernel32.OpenProcess.argtypes = [W.DWORD, W.BOOL, W.DWORD]
kernel32.CloseHandle.argtypes = [ctypes.c_void_p]


# ---------------------------------------------------------------- graphics card
class _FmtValue(ctypes.Structure):
    _fields_ = [('CStatus', W.DWORD), ('doubleValue', ctypes.c_double)]

class _FmtItem(ctypes.Structure):
    _fields_ = [('szName', W.LPWSTR), ('FmtValue', _FmtValue)]

PDH_FMT_DOUBLE, PDH_FMT_NOCAP100, PDH_MORE_DATA = 0x200, 0x8000, 0x800007D2

class GpuLoad:
    """% of the graphics card per process id (its busiest engine, like Task Manager), and the card's busiest engine
    overall. sample() returns {'by_pid': {pid: %}, 'total': %} for the time since the last sample, or None."""
    def __init__(self):
        self.query = self.counter = None
        try:
            pdh = self.pdh = ctypes.windll.pdh
            q, c = ctypes.c_void_p(), ctypes.c_void_p()
            if pdh.PdhOpenQueryW(None, None, ctypes.byref(q)) != 0: return
            if pdh.PdhAddEnglishCounterW(q, '\\GPU Engine(*)\\Utilization Percentage', None, ctypes.byref(c)) != 0:
                pdh.PdhCloseQuery(q); return
            pdh.PdhCollectQueryData(q)                       # the first read: the next one averages from here
            self.query, self.counter, self.fresh = q, c, True
        except (OSError, AttributeError):
            self.query = None

    def sample(self):
        if not self.query: return None
        pdh = self.pdh
        if pdh.PdhCollectQueryData(self.query) != 0: return None
        if self.fresh: self.fresh = False; return None      # made just now: nothing measured over a minute yet
        size, count = W.DWORD(0), W.DWORD(0)
        flags = PDH_FMT_DOUBLE | PDH_FMT_NOCAP100
        r = pdh.PdhGetFormattedCounterArrayW(self.counter, flags, ctypes.byref(size), ctypes.byref(count), None)
        if r & 0xFFFFFFFF != PDH_MORE_DATA or not size.value: return None
        buf = (ctypes.c_byte * size.value)()
        if pdh.PdhGetFormattedCounterArrayW(self.counter, flags, ctypes.byref(size), ctypes.byref(count), buf) != 0: return None
        items = ctypes.cast(buf, ctypes.POINTER(_FmtItem))
        per = {}; engines = {}                               # (pid, engine type) -> %; engine type -> % (all programs)
        for i in range(count.value):
            name, value = items[i].szName or '', items[i].FmtValue.doubleValue
            m = re.match(r'pid_(\d+)_.*_engtype_(\w+)', name)
            if not m or value <= 0: continue
            pid, eng = int(m.group(1)), m.group(2)
            per[(pid, eng)] = per.get((pid, eng), 0.0) + value
            engines[eng] = engines.get(eng, 0.0) + value
        by_pid = {}
        for (pid, _), v in per.items(): by_pid[pid] = max(by_pid.get(pid, 0.0), v)
        return {'by_pid': by_pid, 'total': min(100.0, max(engines.values(), default=0.0))}


# ---------------------------------------------------------------- memory
class _MemStatus(ctypes.Structure):
    _fields_ = [('dwLength', W.DWORD), ('dwMemoryLoad', W.DWORD), ('ullTotalPhys', ctypes.c_uint64),
                ('ullAvailPhys', ctypes.c_uint64), ('ullTotalPageFile', ctypes.c_uint64), ('ullAvailPageFile', ctypes.c_uint64),
                ('ullTotalVirtual', ctypes.c_uint64), ('ullAvailVirtual', ctypes.c_uint64), ('ullAvailExtendedVirtual', ctypes.c_uint64)]

class _ProcMem(ctypes.Structure):
    _fields_ = [('cb', W.DWORD), ('PageFaultCount', W.DWORD), ('PeakWorkingSetSize', ctypes.c_size_t),
                ('WorkingSetSize', ctypes.c_size_t), ('QuotaPeakPagedPoolUsage', ctypes.c_size_t),
                ('QuotaPagedPoolUsage', ctypes.c_size_t), ('QuotaPeakNonPagedPoolUsage', ctypes.c_size_t),
                ('QuotaNonPagedPoolUsage', ctypes.c_size_t), ('PagefileUsage', ctypes.c_size_t), ('PeakPagefileUsage', ctypes.c_size_t)]

def ram():
    """(free GB, total GB) or None."""
    s = _MemStatus(); s.dwLength = ctypes.sizeof(s)
    if not kernel32.GlobalMemoryStatusEx(ctypes.byref(s)): return None
    return s.ullAvailPhys / 2**30, s.ullTotalPhys / 2**30

def process_mb(pid):
    """A process's working set in MB, or None."""
    if not pid: return None
    h = kernel32.OpenProcess(0x1000 | 0x0010, False, pid)      # QUERY_LIMITED_INFORMATION | VM_READ
    if not h: return None
    try:
        m = _ProcMem(); m.cb = ctypes.sizeof(m)
        ok = ctypes.windll.psapi.GetProcessMemoryInfo(ctypes.c_void_p(h), ctypes.byref(m), m.cb)
        return m.WorkingSetSize / 2**20 if ok else None
    finally:
        kernel32.CloseHandle(h)

def sanctum_pid():
    """Sanctum Helper's process id from its status file (hosted by the aLogs app), or 0."""
    try:
        with open(os.path.join(os.environ.get('LOCALAPPDATA', ''), 'SanctumHelper', 'hosted', 'status.json'), encoding='utf-8') as f:
            return int(json.load(f).get('pid') or 0)
    except (OSError, ValueError, TypeError):
        return 0


# ---------------------------------------------------------------- the PC, once
class _DisplayDevice(ctypes.Structure):
    _fields_ = [('cb', W.DWORD), ('DeviceName', W.WCHAR * 32), ('DeviceString', W.WCHAR * 128), ('StateFlags', W.DWORD),
                ('DeviceID', W.WCHAR * 128), ('DeviceKey', W.WCHAR * 128)]

class _DevMode(ctypes.Structure):
    _fields_ = [('dmDeviceName', W.WCHAR * 32), ('dmSpecVersion', W.WORD), ('dmDriverVersion', W.WORD), ('dmSize', W.WORD),
                ('dmDriverExtra', W.WORD), ('dmFields', W.DWORD), ('dmPositionX', W.LONG), ('dmPositionY', W.LONG),
                ('dmDisplayOrientation', W.DWORD), ('dmDisplayFixedOutput', W.DWORD), ('dmColor', W.SHORT),
                ('dmDuplex', W.SHORT), ('dmYResolution', W.SHORT), ('dmTTOption', W.SHORT), ('dmCollate', W.SHORT),
                ('dmFormName', W.WCHAR * 32), ('dmLogPixels', W.WORD), ('dmBitsPerPel', W.DWORD), ('dmPelsWidth', W.DWORD),
                ('dmPelsHeight', W.DWORD), ('dmDisplayFlags', W.DWORD), ('dmDisplayFrequency', W.DWORD)]

def _graphics_cards():
    names, i = [], 0
    while True:
        d = _DisplayDevice(); d.cb = ctypes.sizeof(d)
        if not user32.EnumDisplayDevicesW(None, i, ctypes.byref(d), 0): break
        if d.DeviceString and d.DeviceString not in names and d.StateFlags & 0x1: names.append(d.DeviceString)   # attached
        i += 1
    return names

def _cpu():
    try:
        with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r'HARDWARE\DESCRIPTION\System\CentralProcessor\0') as k:
            return ' '.join(str(winreg.QueryValueEx(k, 'ProcessorNameString')[0]).split())
    except OSError:
        return '?'

def _window_mode(hwnd):
    """How the game's window sits on its monitor: fullscreen / borderless / windowed (and its size)."""
    if not hwnd: return '?'
    if user32.IsIconic(hwnd): return 'minimized'
    r = W.RECT(); user32.GetWindowRect(hwnd, ctypes.byref(r))
    mon = user32.MonitorFromWindow(hwnd, 2)
    class _MonInfo(ctypes.Structure):
        _fields_ = [('cbSize', W.DWORD), ('rcMonitor', W.RECT), ('rcWork', W.RECT), ('dwFlags', W.DWORD)]
    mi = _MonInfo(); mi.cbSize = ctypes.sizeof(mi); user32.GetMonitorInfoW(mon, ctypes.byref(mi))
    covers = (r.left, r.top, r.right, r.bottom) == (mi.rcMonitor.left, mi.rcMonitor.top, mi.rcMonitor.right, mi.rcMonitor.bottom)
    style = user32.GetWindowLongW(hwnd, -16)
    framed = bool(style & 0x00C00000)                          # WS_CAPTION
    mode = 'windowed' if framed or not covers else 'fullscreen or borderless'
    return f'{mode} {r.right - r.left}x{r.bottom - r.top}'

def system_line(hwnd=None):
    """One line on the PC: graphics card(s), CPU and cores, RAM, the main monitor's mode, the game window."""
    dm = _DevMode(); dm.dmSize = ctypes.sizeof(dm)
    monitor = (f'{dm.dmPelsWidth}x{dm.dmPelsHeight} at {dm.dmDisplayFrequency} Hz'
               if user32.EnumDisplaySettingsW(None, -1, ctypes.byref(dm)) else '?')
    mem = ram()
    return (f"PC: graphics {', '.join(_graphics_cards()) or '?'} | CPU {_cpu()}, {os.cpu_count()} threads | "
            f"RAM {mem[1]:.0f} GB | monitor {monitor} | game window {_window_mode(hwnd)}" if mem else 'PC: ?')
