"""The game frame, from the aLogs app's shared capture (app/src-tauri/src/capture.rs).

The app captures the game once for every module, on the graphics card (window capture or Desktop Duplication),
and copies off the card only the areas the modules ask for. The meter never captures by itself any more:
PrintWindow and screen copies took ~50 ms per 4K frame and could make the game stutter.
"""
import ctypes, ctypes.wintypes as W
from PIL import Image

user32 = ctypes.windll.user32; gdi32 = ctypes.windll.gdi32
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)   # real pixels on scaled displays
except Exception: pass

def find_window(title='Aniimo'):
    """The game's window: visible, exactly this title, and a Unity player window (so a File Explorer folder or a
    browser tab called "Aniimo" is never taken for the game, like the app's own capture). Largest if several."""
    want = (title or '').strip().casefold() or 'aniimo'    # like the app's find_game: trimmed, any case, never empty
    best = [(0, None)]
    @ctypes.WINFUNCTYPE(ctypes.c_bool, W.HWND, W.LPARAM)
    def cb(h, _):
        n = user32.GetWindowTextLengthW(h)
        if n and user32.IsWindowVisible(h):
            b = ctypes.create_unicode_buffer(n + 1); user32.GetWindowTextW(h, b, n + 1)
            c = ctypes.create_unicode_buffer(64); user32.GetClassNameW(h, c, 64)
            if b.value.strip().casefold() == want and c.value == 'UnityWndClass':
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

class SharedCapture:
    r"""Reader for the aLogs app's shared game picture (`Local\aLogs.Capture.v2`, layout in capture.rs).
    It asks for pictures at the rate it is called and for the area it needs (the box around everything it read in
    the last couple of seconds; only that leaves the graphics card), and gets them in its own picture buffer."""
    NAME, MAGIC, HEADER, SLOTS, SLOT_BASE, PICTURE_BASE, PICTURE = r'Local\aLogs.Capture.v2', 0x50434C41, 4096, 16, 64, 320, 64
    SOURCES = {1: 'window copy (tabbed out)', 3: 'Desktop Duplication', 4: 'window capture'}
    AREA_KEEP = 2.0            # seconds an area stays asked for after its last use

    def __init__(self, tag=0):
        """`tag` > 0: a second reader in this process with a slot of its own (e.g. the damage-number recorder's middle
        of the screen next to the meter's top band); slots are told apart by process id with the tag in the top bits."""
        import os
        self.owner = os.getpid() | (tag << 28)
        self.base = None; self.slot = None; self.next_open = 0.0; self.last_call = 0.0; self.interval = 250.0
        self.screen_copy = False; self.copy_us = 0     # the last picture: our overlays may be in it (flags bit 1), copy time
        self.source = ''                               # where the last picture came from (flags bits 4-7)
        self.areas = []; self.box = None; self.request = 0
        k = self.k = ctypes.windll.kernel32
        k.OpenFileMappingW.restype = W.HANDLE; k.OpenFileMappingW.argtypes = [W.DWORD, W.BOOL, W.LPCWSTR]
        k.MapViewOfFile.restype = ctypes.c_void_p
        k.MapViewOfFile.argtypes = [W.HANDLE, W.DWORD, W.DWORD, W.DWORD, ctypes.c_size_t]
        k.UnmapViewOfFile.argtypes = [ctypes.c_void_p]; k.CloseHandle.argtypes = [W.HANDLE]
        self.handle = None

    HEARTBEAT, HEARTBEAT_STALE = 2048, 5000   # the app's capture thread stamps this; older than 5 s: the app is gone

    def _close(self):
        if self.base: self.k.UnmapViewOfFile(self.base)
        if self.handle: self.k.CloseHandle(self.handle)
        self.base = self.handle = None; self.slot = None

    def _open(self, now):
        if self.base or now < self.next_open: return self.base
        self.next_open = now + 2.0
        h = self.k.OpenFileMappingW(0xF001F, False, self.NAME)      # FILE_MAP_ALL_ACCESS: we write our request slot
        if not h: return None
        base = self.k.MapViewOfFile(h, 0xF001F, 0, 0, 0)            # the whole mapping
        if not base or self._u32(base, 0) != self.MAGIC:
            if base: self.k.UnmapViewOfFile(base)
            self.k.CloseHandle(h); return None
        self.base, self.handle = base, h
        return base

    @staticmethod
    def _u16(base, at): return ctypes.c_uint16.from_address(base + at).value
    @staticmethod
    def _u32(base, at): return ctypes.c_uint32.from_address(base + at).value
    @staticmethod
    def _u64(base, at): return ctypes.c_uint64.from_address(base + at).value

    def _request(self, base, now_ms):
        pid = self.owner
        if self.slot is None or self._u32(base, self.SLOT_BASE + self.slot * 16 + 12) != pid:
            free = [i for i in range(self.SLOTS) if self._u64(base, self.SLOT_BASE + i * 16) < now_ms
                    or self._u32(base, self.SLOT_BASE + i * 16 + 12) == pid]
            if not free: return False
            self.slot = free[0]; self.box = None
            ctypes.c_uint8.from_address(base + self.PICTURE_BASE + self.slot * self.PICTURE + 60).value = 0   # no shrink (the slot's last owner may have asked)
        at = self.SLOT_BASE + self.slot * 16
        ctypes.c_uint32.from_address(base + at + 12).value = pid
        ctypes.c_uint32.from_address(base + at + 8).value = int(self.interval)
        ctypes.c_uint64.from_address(base + at).value = now_ms + 2500
        return True

    def _ask_area(self, base, now, area):
        """Asks for the box around `area` and the areas used in the last AREA_KEEP seconds (shares of the frame,
        0-65535); a new box gets a new request id."""
        self.areas = [(t, a) for t, a in self.areas if now - t < self.AREA_KEEP and a != area] + [(now, area)]
        box = tuple(f(a[k] for _, a in self.areas) for k, f in enumerate((min, min, max, max)))
        at = self.PICTURE_BASE + self.slot * self.PICTURE
        if box != self.box or self._u32(base, at + 8) != self.request:
            self.box = box; self.request = (self.request % 0xFFFFFFFF) + 1
            for k, v in enumerate(box): ctypes.c_uint16.from_address(base + at + 2 * k).value = v
            ctypes.c_uint32.from_address(base + at + 8).value = self.request

    def read(self, hwnd, top_fraction=None):
        """The newest picture of this window (its top `top_fraction`, or all of it) as a PIL image, or None (app not
        running, our area not served yet, picture stale or invalid)."""
        import time
        now = time.time()
        if self.last_call:   # follow the caller's pace: 30/s in fights, 20/s otherwise
            self.interval = min(1000.0, max(25.0, 0.7 * self.interval + 0.3 * (now - self.last_call) * 1000))
        self.last_call = now
        base = self._open(now)
        if not base: return None
        now_ms = int(now * 1000)
        if not self._request(base, now_ms): return None
        top = 65535 if not top_fraction else min(65535, int(65535 * top_fraction) + 1)
        self._ask_area(base, now, (0, 0, 65535, top))
        if self._u64(base, 40) != (hwnd or 0): return None
        at = self.PICTURE_BASE + self.slot * self.PICTURE
        for _ in range(4):
            seq = self._u64(base, at + 16)
            if seq & 1: time.sleep(0.002); continue
            if self._u32(base, at + 12) != self.request: return None              # our area isn't in yet
            flags = self._u32(base, at + 48)
            # tabbed out on Windows 10 the app copies the window only 4 times a second (source 1): older is fine then
            fresh = 600 if flags >> 4 & 15 == 1 else max(150, 2 * self.interval)
            if not flags & 1 or now_ms - self._u64(base, at + 24) > fresh: return None
            if ctypes.c_uint8.from_address(base + at + 61).value > 1: return None  # shrunk: not what we asked for
            offset, stride = self._u32(base, at + 32), self._u32(base, at + 36)
            x, y, w, h = (self._u16(base, at + 40 + 2 * k) for k in range(4))
            cw, ch = self._u16(base, at + 56), self._u16(base, at + 58)
            rows = ch if not top_fraction else min(ch, int(ch * top_fraction))
            if x + w > cw or y + h > ch or not w or not h: return None
            # converted straight out of the shared memory into the image's own pixels; a picture written
            # meanwhile shows in the sequence number and is read again
            got = min(h, max(0, rows - y))
            im = Image.frombuffer('RGB', (w, got), (ctypes.c_char * (stride * got)).from_address(base + self.HEADER + offset),
                                  'raw', 'BGRX', stride, 1) if got else None
            if im is not None: im.load()
            if (x, y, w, got) != (0, 0, cw, rows):
                # the game window is partly off its screen (Desktop Duplication sees only the screen): what's on it in
                # place, the rest black, so every reader's coordinates stay right
                full = Image.new('RGB', (cw, rows))
                if im is not None: full.paste(im, (x, y))
                im = full
            if self._u64(base, at + 16) == seq:
                self.screen_copy, self.copy_us = bool(flags & 2), self._u32(base, at + 52)
                self.source = self.SOURCES.get(flags >> 4 & 15, '?')
                return im
        return None

    def read_area(self, hwnd, area, shrink=1):
        """The part `area` (x0, y0, x1, y1 as shares of the client, 0-1) of the newest picture, as a PIL image, or None.
        For a reader of its own (tag > 0) asking for one fixed area, e.g. the middle of the screen. `shrink` 2 or 4:
        the graphics card averages it down to half or a quarter first (only that leaves the card; an app without
        that gives it full size, see the image's size)."""
        import time
        now = time.time()
        if self.last_call: self.interval = min(1000.0, max(16.0, 0.7 * self.interval + 0.3 * (now - self.last_call) * 1000))
        self.last_call = now
        base = self._open(now)
        if not base or not self._request(base, int(now * 1000)): return None
        want = tuple(min(65535, max(0, int(v * 65535))) for v in area)
        at = self.PICTURE_BASE + self.slot * self.PICTURE
        if ctypes.c_uint8.from_address(base + at + 60).value != shrink:
            ctypes.c_uint8.from_address(base + at + 60).value = shrink; self.box = None     # a new request
        self._ask_area(base, now, want)
        if self._u64(base, 40) != (hwnd or 0): return None
        at = self.PICTURE_BASE + self.slot * self.PICTURE
        for _ in range(4):
            seq = self._u64(base, at + 16)
            if seq & 1: time.sleep(0.001); continue
            if self._u32(base, at + 12) != self.request: return None
            flags = self._u32(base, at + 48)
            if not flags & 1 or int(now * 1000) - self._u64(base, at + 24) > max(100, 2 * self.interval): return None
            offset, stride = self._u32(base, at + 32), self._u32(base, at + 36)
            x, y, w, h = (self._u16(base, at + 40 + 2 * k) for k in range(4))
            used = ctypes.c_uint8.from_address(base + at + 61).value or 1
            w, h = w // used, h // used
            pixels = (ctypes.c_char * (stride * h)).from_address(base + self.HEADER + offset)
            im = Image.frombuffer('RGB', (w, h), pixels, 'raw', 'BGRX', stride, 1)
            im.load()
            if self._u64(base, at + 16) == seq:
                self.picture_time = self._u64(base, at + 24) / 1000      # when the app took it (to tell frames apart)
                return im
        return None

    def switched_off(self):
        """The player turned screen capture off (the overlay button or the app): nothing is captured, the meter rests."""
        import time
        base = self._open(time.time())
        return bool(base and self._u32(base, 60) == 1)

    def app_present(self):
        """The aLogs app is running and sharing its capture (the meter only takes pictures from it). A mapping whose
        heartbeat stopped (the app was closed) is let go, so a new app's is found."""
        import time
        now = time.time(); base = self._open(now)
        if base:
            beat = self._u64(base, self.HEARTBEAT)
            if beat and now * 1000 - beat > self.HEARTBEAT_STALE:
                self._close(); return False
        return bool(base)

shared = SharedCapture()

class SharedAudio:
    r"""Reader for the aLogs app's audio cues (`LocalLogs.Audio.v1`, layout in audio.rs). want() keeps the app
    listening to the game for a few more seconds; cue() is the newest boss kill sound."""
    NAME, MAGIC, SIZE = r'LocalLogs.Audio.v1', 0x55414C41, 4096

    def __init__(self):
        self.base = None; self.next_open = 0.0

    def _open(self, now):
        if self.base or now < self.next_open: return self.base
        self.next_open = now + 2.0
        k = SharedCapture().k
        h = k.OpenFileMappingW(0xF001F, False, self.NAME)
        if not h: return None
        base = k.MapViewOfFile(h, 0xF001F, 0, 0, self.SIZE)
        if not base or SharedCapture._u32(base, 0) != self.MAGIC: return None
        self.base = base
        return base

    def want(self, now, seconds=3.0):
        base = self._open(now)
        if base: ctypes.c_uint64.from_address(base + 8).value = int((now + seconds) * 1000)
        return bool(base)

    def state(self):
        """0 not listening, 1 listening, 2 can't listen (no game, or Windows refused), None without the app."""
        return SharedCapture._u32(self.base, 32) if self.base else None

    def cue(self):
        """(count, unix time the sound started, match score) of the newest boss kill sound, or None."""
        if not self.base: return None
        n = SharedCapture._u32(self.base, 28)
        if not n: return None
        return n, SharedCapture._u64(self.base, 16) / 1000, ctypes.c_float.from_address(self.base + 24).value

audio = SharedAudio()

def grab_window(hwnd, top_fraction=None):
    """The game's client area (or just its top part) as a PIL image, or None: the app's shared frame, None until
    the app has a fresh one (or isn't running)."""
    try: return shared.read(hwnd, top_fraction) if shared.app_present() else None
    except Exception: return None

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
