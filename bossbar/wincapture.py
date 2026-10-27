"""Window capture with built-in Windows GDI calls (same approach as Sanctum Helper).

Shared: while the aLogs app runs it copies the game window once for every module (app/src-tauri/src/capture.rs);
grab_window reads that frame first and only captures by itself when the app isn't there.
Primary: PrintWindow(hwnd, PW_CLIENTONLY | PW_RENDERFULLCONTENT) - renders the game window into a bitmap,
works when the window is covered, no yellow border (it is not Windows Graphics Capture).
Fallback: BitBlt from the screen (the game must be visible).
"""
import ctypes, ctypes.wintypes as W
from PIL import Image

user32 = ctypes.windll.user32; gdi32 = ctypes.windll.gdi32
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)   # real pixels on scaled displays
except Exception: pass

PW_CLIENTONLY, PW_RENDERFULLCONTENT = 0x1, 0x2
SRCCOPY, DIB_RGB_COLORS = 0x00CC0020, 0

class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [('biSize', W.DWORD), ('biWidth', W.LONG), ('biHeight', W.LONG), ('biPlanes', W.WORD),
                ('biBitCount', W.WORD), ('biCompression', W.DWORD), ('biSizeImage', W.DWORD),
                ('biXPelsPerMeter', W.LONG), ('biYPelsPerMeter', W.LONG), ('biClrUsed', W.DWORD), ('biClrImportant', W.DWORD)]

user32.PrintWindow.argtypes = [W.HWND, W.HDC, W.UINT]
gdi32.GetDIBits.argtypes = [W.HDC, W.HBITMAP, W.UINT, W.UINT, ctypes.c_void_p, ctypes.c_void_p, W.UINT]
# handles are 64-bit: without these, ctypes treats them as 32-bit ints and a large one fails ("int too long")
user32.GetDC.argtypes = [W.HWND]; user32.GetDC.restype = W.HDC
user32.ReleaseDC.argtypes = [W.HWND, W.HDC]
gdi32.CreateCompatibleDC.argtypes = [W.HDC]; gdi32.CreateCompatibleDC.restype = W.HDC
gdi32.CreateCompatibleBitmap.argtypes = [W.HDC, ctypes.c_int, ctypes.c_int]; gdi32.CreateCompatibleBitmap.restype = W.HBITMAP
gdi32.SelectObject.argtypes = [W.HDC, W.HGDIOBJ]; gdi32.SelectObject.restype = W.HGDIOBJ
gdi32.DeleteObject.argtypes = [W.HGDIOBJ]
gdi32.DeleteDC.argtypes = [W.HDC]
gdi32.BitBlt.argtypes = [W.HDC, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, W.HDC, ctypes.c_int, ctypes.c_int, W.DWORD]

def find_window(title='Aniimo'):
    """The game's window: visible, exactly this title, and a Unity player window (so a File Explorer folder or a
    browser tab called "Aniimo" is never taken for the game, like the app's own capture). Largest if several."""
    best = [(0, None)]
    @ctypes.WINFUNCTYPE(ctypes.c_bool, W.HWND, W.LPARAM)
    def cb(h, _):
        n = user32.GetWindowTextLengthW(h)
        if n and user32.IsWindowVisible(h):
            b = ctypes.create_unicode_buffer(n + 1); user32.GetWindowTextW(h, b, n + 1)
            c = ctypes.create_unicode_buffer(64); user32.GetClassNameW(h, c, 64)
            if b.value == title and c.value == 'UnityWndClass':
                r = W.RECT(); user32.GetClientRect(h, ctypes.byref(r))
                area = r.right * r.bottom
                if area > best[0][0]: best[0] = (area, h)
        return True
    user32.EnumWindows(cb, 0)
    return best[0][1]

def client_rect(hwnd):
    r = W.RECT(); user32.GetClientRect(hwnd, ctypes.byref(r))
    p = W.POINT(0, 0); user32.ClientToScreen(hwnd, ctypes.byref(p))
    return p.x, p.y, r.right, r.bottom

def _bitmap_to_image(hdc, hbm, w, h, rows=None):
    """The bitmap as a PIL image; with `rows`, only the top `rows` lines (cheaper at 4K, same coordinates)."""
    rows = h if not rows or rows >= h else int(rows)
    # bottom-up DIB: the top `rows` lines are scan lines h-rows .. h-1
    bmi = BITMAPINFOHEADER(ctypes.sizeof(BITMAPINFOHEADER), w, h, 1, 32, 0, 0, 0, 0, 0, 0)
    buf = ctypes.create_string_buffer(w * rows * 4)
    gdi32.GetDIBits(hdc, hbm, h - rows, rows, buf, ctypes.byref(bmi), DIB_RGB_COLORS)
    return Image.frombuffer('RGB', (w, rows), buf, 'raw', 'BGRX', 0, -1)

class SharedCapture:
    r"""Reader for the aLogs app's shared frame (`Local\aLogs.Capture.v1`, layout in capture.rs).
    It asks for frames at the rate it is called and takes the newest complete one."""
    NAME, MAGIC, HEADER, SLOTS, SLOT_BASE, SIZE = r'Local\aLogs.Capture.v1',0x50434C41, 4096, 16, 64, 4096 + 3840 * 2160 * 4

    def __init__(self):
        self.base = None; self.slot = None; self.next_open = 0.0; self.last_call = 0.0; self.interval = 250.0
        k = self.k = ctypes.windll.kernel32
        k.OpenFileMappingW.restype = W.HANDLE; k.OpenFileMappingW.argtypes = [W.DWORD, W.BOOL, W.LPCWSTR]
        k.MapViewOfFile.restype = ctypes.c_void_p
        k.MapViewOfFile.argtypes = [W.HANDLE, W.DWORD, W.DWORD, W.DWORD, ctypes.c_size_t]

    def _open(self, now):
        if self.base or now < self.next_open: return self.base
        self.next_open = now + 2.0
        h = self.k.OpenFileMappingW(0xF001F, False, self.NAME)      # FILE_MAP_ALL_ACCESS: we write our request slot
        if not h: return None
        base = self.k.MapViewOfFile(h, 0xF001F, 0, 0, self.SIZE)
        if not base or self._u32(base, 0) != self.MAGIC: return None
        self.base = base
        return base

    @staticmethod
    def _u32(base, at): return ctypes.c_uint32.from_address(base + at).value
    @staticmethod
    def _u64(base, at): return ctypes.c_uint64.from_address(base + at).value

    def _request(self, base, now_ms):
        import os
        pid = os.getpid()
        if self.slot is None or self._u32(base, self.SLOT_BASE + self.slot * 16 + 12) != pid:
            free = [i for i in range(self.SLOTS) if self._u64(base, self.SLOT_BASE + i * 16) < now_ms
                    or self._u32(base, self.SLOT_BASE + i * 16 + 12) == pid]
            if not free: return False
            self.slot = free[0]
        at = self.SLOT_BASE + self.slot * 16
        ctypes.c_uint32.from_address(base + at + 12).value = pid
        ctypes.c_uint32.from_address(base + at + 8).value = int(self.interval)
        ctypes.c_uint64.from_address(base + at).value = now_ms + 2500
        return True

    def read(self, hwnd, top_fraction=None):
        """The newest shared frame of this window as a PIL image, or None (app not running, frame stale or invalid)."""
        import time
        now = time.time()
        if self.last_call:   # follow the caller's pace: 30/s in fights, 20/s otherwise
            self.interval = min(1000.0, max(25.0, 0.7 * self.interval + 0.3 * (now - self.last_call) * 1000))
        self.last_call = now
        base = self._open(now)
        if not base: return None
        now_ms = int(now * 1000)
        if not self._request(base, now_ms): return None
        for _ in range(4):
            seq = self._u64(base, 8)
            if seq & 1: time.sleep(0.002); continue
            w, h, stride, flags = (self._u32(base, at) for at in (16, 20, 24, 28))
            frame_ms, frame_hwnd = self._u64(base, 32), self._u64(base, 40)
            if not flags & 1 or frame_hwnd != (hwnd or 0) or now_ms - frame_ms > max(150, 2 * self.interval): return None
            rows = h if not top_fraction else min(h, int(h * top_fraction))
            # converted straight out of the shared memory into the image's own pixels (no extra copy of the raw
            # frame first); a frame written meanwhile shows in the sequence number and is read again
            im = Image.frombuffer('RGB', (w, rows), (ctypes.c_char * (stride * rows)).from_address(base + self.HEADER),
                                  'raw', 'BGRX', stride, 1)
            im.load()
            if self._u64(base, 8) == seq:
                return im
        return None

shared = SharedCapture()

def grab_window(hwnd, top_fraction=None):
    """The game's client area (or just its top part) as a PIL image, or None: the app's shared frame when it is
    fresh, else our own PrintWindow capture."""
    try:
        im = shared.read(hwnd, top_fraction)
        if im is not None: return im
    except Exception:
        pass
    _, _, w, h = client_rect(hwnd)
    if w <= 0 or h <= 0: return None
    hdc_win = user32.GetDC(hwnd); hdc = gdi32.CreateCompatibleDC(hdc_win)
    hbm = gdi32.CreateCompatibleBitmap(hdc_win, w, h); old = gdi32.SelectObject(hdc, hbm)
    try:
        ok = user32.PrintWindow(hwnd, hdc, PW_CLIENTONLY | PW_RENDERFULLCONTENT)
        return _bitmap_to_image(hdc, hbm, w, h, h * top_fraction if top_fraction else None) if ok else None
    finally:
        gdi32.SelectObject(hdc, old); gdi32.DeleteObject(hbm); gdi32.DeleteDC(hdc); user32.ReleaseDC(hwnd, hdc_win)

def grab_screen(x, y, w, h):
    """BitBlt from the screen (what is visible at that spot)."""
    hdc_scr = user32.GetDC(None); hdc = gdi32.CreateCompatibleDC(hdc_scr)
    hbm = gdi32.CreateCompatibleBitmap(hdc_scr, w, h); old = gdi32.SelectObject(hdc, hbm)
    try:
        gdi32.BitBlt(hdc, 0, 0, w, h, hdc_scr, x, y, SRCCOPY)
        return _bitmap_to_image(hdc, hbm, w, h)
    finally:
        gdi32.SelectObject(hdc, old); gdi32.DeleteObject(hbm); gdi32.DeleteDC(hdc); user32.ReleaseDC(None, hdc_scr)

def is_blank(im):
    """True if the image is (nearly) one flat color - what PrintWindow returns when it fails."""
    ext = im.reduce(max(1, im.width // 128)).getextrema()     # cheap: a ~128 px wide copy is enough
    return all(hi - lo < 6 for lo, hi in ext)

WDA_EXCLUDEFROMCAPTURE = 0x11
def hide_from_capture(hwnd):
    """Keep our own overlay window out of screen captures (Windows 10 2004+)."""
    return bool(user32.SetWindowDisplayAffinity(W.HWND(hwnd), WDA_EXCLUDEFROMCAPTURE))

def is_foreground(hwnd):
    return user32.GetForegroundWindow() == hwnd

MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP = 0x2, 0x4
def click_at(x, y):
    """One left click at a screen point, then put the cursor back where it was (built-in SetCursorPos/mouse_event)."""
    import time
    p = W.POINT(); user32.GetCursorPos(ctypes.byref(p))
    user32.SetCursorPos(int(x), int(y)); time.sleep(0.03)
    user32.mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0); time.sleep(0.05)
    user32.mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0); time.sleep(0.03)
    user32.SetCursorPos(p.x, p.y)
