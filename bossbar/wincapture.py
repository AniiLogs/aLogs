"""The game frame, from the aLogs app's shared capture (app/src-tauri/src/capture.rs).

The app captures the game once for every module, on the graphics card (window capture or Desktop Duplication),
and copies off the card only the areas the modules ask for. The meter never captures by itself any more:
PrintWindow and screen copies took ~50 ms per 4K frame and could make the game stutter.
"""
import ctypes, ctypes.wintypes as W
from PIL import Image
import devflags as D

user32 = ctypes.windll.user32; gdi32 = ctypes.windll.gdi32
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)   # real pixels on scaled displays
except Exception: pass

class _MODULEENTRY32W(ctypes.Structure):
    _fields_ = [('dwSize', W.DWORD), ('th32ModuleID', W.DWORD), ('th32ProcessID', W.DWORD), ('GlblcntUsage', W.DWORD),
                ('ProccntUsage', W.DWORD), ('modBaseAddr', ctypes.c_void_p), ('modBaseSize', W.DWORD), ('hModule', W.HMODULE),
                ('szModule', ctypes.c_wchar * 256), ('szExePath', ctypes.c_wchar * 260)]

def window_modules(hwnd):
    """The file names (lower case) of the DLLs loaded in the process that owns hwnd; an empty set when they can't be
    listed. Read-only (a ToolHelp snapshot), for overlays drawn inside the game: those are in every capture."""
    pid = W.DWORD(); user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    k32 = ctypes.windll.kernel32
    k32.CreateToolhelp32Snapshot.restype = W.HANDLE
    snap = k32.CreateToolhelp32Snapshot(0x8 | 0x10, pid.value)          # TH32CS_SNAPMODULE | SNAPMODULE32
    if not snap or snap == W.HANDLE(-1).value: return set()
    out = set()
    try:
        e = _MODULEENTRY32W(); e.dwSize = ctypes.sizeof(e)
        ok = k32.Module32FirstW(snap, ctypes.byref(e))
        while ok:
            out.add(e.szModule.lower()); ok = k32.Module32NextW(snap, ctypes.byref(e))
    finally: k32.CloseHandle(snap)
    return out

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

# how old a whole frame from the app may be (ms): the app keeps the newest finished one readable while the graphics card
# copies the next off, up to ~0.85 s on a busy result screen; the meter's fight reads (its top part) stay strict
WHOLE_FRESH_MS = 1500

class SharedCapture:
    r"""Reader for the aLogs app's shared game picture (`Local\aLogs.Capture.v2`, layout in capture.rs).
    It asks for pictures at the rate it is called and for the area it needs (the box around everything it read in
    the last couple of seconds; only that leaves the graphics card), and gets them in its own picture buffer."""
    NAME, MAGIC, HEADER, SLOTS, SLOT_BASE, PICTURE_BASE, PICTURE = r'Local\aLogs.Capture.v2', 0x50434C41, 4096, 16, 64, 320, 64
    SOURCES = {1: 'window copy (tabbed out)', 3: 'Desktop Duplication', 4: 'window capture'}
    AREA_KEEP = 2.0            # seconds an area stays asked for after its last use
    AREA_SHORT = 0.3           # ...with the Dev switch area-keep-short on
    asked = grown = 0          # area requests made, and how many of them were for more rows (the meter's dev line)
    # seconds read() may wait for its picture. The app copies a reader's picture in place with its seq odd the whole
    # time (10-35 ms for a 4K frame) and serves each reader at the pace it reads, so the meter ran in step with the
    # app and kept arriving mid-copy: 4 tries 2 ms apart gave up on up to half the frames (meter.log 2026-10-30,
    # Desktop Duplication: 146 without a picture a minute at 10 ms copies, 631 at 33 ms; the 3 ms window copy: 0).
    # Waiting returns the picture being written, newer than any it could have had; a new area waits the same way.
    WAIT = 0.12

    def __init__(self, tag=0):
        """`tag` > 0: a second reader in this process with a slot of its own (e.g. the damage-number recorder's middle
        of the screen next to the meter's top band); slots are told apart by process id with the tag in the top bits."""
        import os
        self.owner = os.getpid() | (tag << 28)
        self.base = None; self.slot = None; self.next_open = 0.0; self.last_call = 0.0; self.interval = 250.0
        self.screen_copy = False; self.copy_us = 0     # the last picture: our overlays may be in it (flags bit 1), copy time
        self.why = ''                                  # why the last read gave no picture (_none)
        self.source = ''                               # where the last picture came from (flags bits 4-7)
        self.areas = []; self.box = None; self.request = 0
        self.boxes = {}                                # request id -> box, for our last few requests (read)
        k = self.k = ctypes.windll.kernel32
        k.OpenFileMappingW.restype = W.HANDLE; k.OpenFileMappingW.argtypes = [W.DWORD, W.BOOL, W.LPCWSTR]
        k.MapViewOfFile.restype = ctypes.c_void_p
        k.MapViewOfFile.argtypes = [W.HANDLE, W.DWORD, W.DWORD, W.DWORD, ctypes.c_size_t]
        k.UnmapViewOfFile.argtypes = [ctypes.c_void_p]; k.CloseHandle.argtypes = [W.HANDLE]
        self.handle = None

    RECT_BASE, RECT_SIZE, RECTS = 2056, 120, 7   # several separate parts instead of one box (B8 apps, read_rects)
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

    def _none(self, why):
        """read()'s None, with why (self.why): the meter logs it when pictures stop coming for a while."""
        self.why = why
        return None

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
            self.slot = free[0]; self.box = None; self.boxes = {}   # the slot's picture is someone else's
            ctypes.c_uint8.from_address(base + self.PICTURE_BASE + self.slot * self.PICTURE + 60).value = 0   # no shrink (the slot's last owner may have asked)
        at = self.SLOT_BASE + self.slot * 16
        ctypes.c_uint32.from_address(base + at + 12).value = pid
        ctypes.c_uint32.from_address(base + at + 8).value = int(self.interval)
        ctypes.c_uint64.from_address(base + at).value = now_ms + 2500
        return True

    def _ask_area(self, base, now, area):
        """Asks for the box around `area` and the areas used in the last AREA_KEEP seconds (shares of the frame,
        0-65535); a new box gets a new request id."""
        # Dev switch area-keep-short: a whole frame stays asked for 0.3 s after its last use, not 2 s, so the app stops
        # copying whole frames between screen reads that are further apart (each such read then waits for its own copy)
        keep = self.AREA_SHORT if D.on('area-keep-short') else self.AREA_KEEP
        self.areas = [(t, a) for t, a in self.areas if now - t < keep and a != area] + [(now, area)]
        box = tuple(f(a[k] for _, a in self.areas) for k, f in enumerate((min, min, max, max)))
        at = self.PICTURE_BASE + self.slot * self.PICTURE
        if box != self.box or self._u32(base, at + 8) != self.request:
            self.asked += 1; self.grown += bool(self.box and len(self.box) == 4 and isinstance(self.box[3], int) and box[3] > self.box[3])
            self.box = box; self.request = (self.request % 0xFFFFFFFF) + 1
            ctypes.c_uint32.from_address(base + self.RECT_BASE + self.slot * self.RECT_SIZE).value = 0   # one area, no parts
            for k, v in enumerate(box): ctypes.c_uint16.from_address(base + at + 2 * k).value = v
            ctypes.c_uint32.from_address(base + at + 8).value = self.request
            self.boxes = {r: b for r, b in list(self.boxes.items())[-3:]}; self.boxes[self.request] = box

    def read(self, hwnd, top_fraction=None):
        """The newest picture of this window (its top `top_fraction`, or all of it) as a PIL image, or None (app not
        running, our area not served yet, picture stale or invalid)."""
        import time
        now = time.time()
        if self.last_call:   # follow the caller's pace: 30/s in fights, 20/s otherwise
            self.interval = min(1000.0, max(25.0, 0.7 * self.interval + 0.3 * (now - self.last_call) * 1000))
        self.last_call = now
        base = self._open(now)
        if not base: return self._none('the app shares no picture (not running, or its capture is off)')
        now_ms = int(now * 1000)
        if not self._request(base, now_ms): return self._none('no free reader slot')
        top = 65535 if not top_fraction else min(65535, int(65535 * top_fraction) + 1)
        self._ask_area(base, now, (0, 0, 65535, top))
        if self._u64(base, 40) != (hwnd or 0): return self._none('the app is capturing another window')
        at = self.PICTURE_BASE + self.slot * self.PICTURE
        # tabbed out on Windows 10 the window copy comes 4 times a second: a new area isn't worth waiting for there
        until = now + self.WAIT; area_until = now if self.source == self.SOURCES[1] else until
        while True:
            seq = self._u64(base, at + 16)
            if seq & 1:                                                           # being written: it's nearly done
                if time.time() >= until: return self._none('the picture was being written')
                time.sleep(0.001); continue
            # the picture of an earlier request whose box holds all we want does too: a box shrinking after a whole
            # frame (AREA_KEEP) lost a frame each time, and between fights that flip-flopped every few seconds
            box = self.boxes.get(self._u32(base, at + 12))
            if box is None or box[:3] != (0, 0, 65535) or box[3] < top:          # our area isn't in yet
                if time.time() >= area_until: return self._none('our area is not in the picture yet')
                time.sleep(0.002); continue
            flags = self._u32(base, at + 48)
            # tabbed out on Windows 10 the app copies the window only 4 times a second (source 1): older is fine then
            fresh = 600 if flags >> 4 & 15 == 1 else max(150, 2 * self.interval)
            # a whole frame (screens: menus, results, Battle Data): the app hands it over only once the graphics card has
            # copied it off, which with the game keeping the card busy took 500-850 ms on an Interlink result (7 Nov)
            if not top_fraction: fresh = max(fresh, WHOLE_FRESH_MS)
            if not flags & 1 or now_ms - self._u64(base, at + 24) > fresh: return self._none('no fresh picture from the app (no valid frame, or an old one)')
            if ctypes.c_uint8.from_address(base + at + 61).value > 1: return self._none('the picture came shrunk')  # shrunk: not what we asked for
            offset, stride = self._u32(base, at + 32), self._u32(base, at + 36)
            x, y, w, h = (self._u16(base, at + 40 + 2 * k) for k in range(4))
            cw, ch = self._u16(base, at + 56), self._u16(base, at + 58)
            rows = ch if not top_fraction else min(ch, int(ch * top_fraction))
            if x + w > cw or y + h > ch or not w or not h: return self._none('the picture does not fit the window')
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
                self._times(base, at)
                return im
            if time.time() >= until: return self._none('the picture was rewritten while it was read')                                 # rewritten while we read it

    def _times(self, base, at):
        self.picture_time = self._u64(base, at + 24) / 1000      # when the app took it (to tell frames apart)
        # when the game showed it: the app writes the image's age then (window capture's own frame time; 0
        # when the source doesn't say), so fight times follow the game, not when the meter got to reading
        self.frame_time = self.picture_time - self._u16(base, at + 62) / 1000

    @classmethod
    def merge_rects(cls, rects, most=None):
        """`rects` (shares 0-65535: x0, y0, x1, y1) cut to `most` (RECTS) by joining the two whose joined box adds
        the least area, until few enough."""
        most = most or cls.RECTS; rs = [tuple(r) for r in rects]
        area = lambda r: (r[2] - r[0]) * (r[3] - r[1])
        join = lambda a, b: (min(a[0], b[0]), min(a[1], b[1]), max(a[2], b[2]), max(a[3], b[3]))
        while len(rs) > most:
            i, j = min(((i, j) for i in range(len(rs)) for j in range(i + 1, len(rs))),
                       key=lambda ij: area(join(rs[ij[0]], rs[ij[1]])) - area(rs[ij[0]]) - area(rs[ij[1]]))
            rs[i] = join(rs[i], rs[j]); del rs[j]
        return rs

    def read_rects(self, hwnd, rects):
        """Only the parts `rects` (shares of the client, 0-1: x0, y0, x1, y1; joined down to RECTS) of the newest
        picture, pasted in place on a black frame of the client's size, as a PIL image, or None. Much less leaves the
        graphics card than the whole frame. An app from before B8 sends the box around them all instead (same result)."""
        import time
        now = time.time()
        if self.last_call: self.interval = min(1000.0, max(25.0, 0.7 * self.interval + 0.3 * (now - self.last_call) * 1000))
        self.last_call = now
        base = self._open(now)
        if not base or not self._request(base, int(now * 1000)): return self._none('the app shares no picture (not running, or no free reader slot)')
        want = tuple(sorted(self.merge_rects([tuple(min(65535, max(0, int(v * 65535) + (k >= 2))) for k, v in enumerate(r))
                                              for r in rects])))
        at = self.PICTURE_BASE + self.slot * self.PICTURE; ra = self.RECT_BASE + self.slot * self.RECT_SIZE
        if want != self.box or self._u32(base, at + 8) != self.request or self._u32(base, ra) != len(want):
            self.box = want; self.request = (self.request % 0xFFFFFFFF) + 1
            ctypes.c_uint8.from_address(base + at + 60).value = 0
            for k, r in enumerate(want):
                for j, v in enumerate(r): ctypes.c_uint16.from_address(base + ra + 8 + 16 * k + 2 * j).value = v
            ctypes.c_uint32.from_address(base + ra).value = len(want)
            whole = tuple(f(r[k] for r in want) for k, f in enumerate((min, min, max, max)))   # for apps before B8
            for k, v in enumerate(whole): ctypes.c_uint16.from_address(base + at + 2 * k).value = v
            ctypes.c_uint32.from_address(base + at + 8).value = self.request
            self.boxes = {r: b for r, b in list(self.boxes.items())[-3:]}; self.boxes[self.request] = want
        if self._u64(base, 40) != (hwnd or 0): return self._none('the app is capturing another window')
        until = now + self.WAIT
        while True:
            seq = self._u64(base, at + 16)
            if seq & 1 or self._u32(base, at + 12) != self.request:      # being written, or our parts aren't in yet
                if time.time() >= until: return self._none('the picture was being written')
                time.sleep(0.002 if not seq & 1 else 0.001); continue
            flags = self._u32(base, at + 48)
            fresh = 600 if flags >> 4 & 15 == 1 else max(150, 2 * self.interval)
            if not flags & 1 or int(time.time() * 1000) - self._u64(base, at + 24) > fresh: return self._none('no fresh picture from the app (no valid frame, or an old one)')
            if ctypes.c_uint8.from_address(base + at + 61).value > 1: return self._none('the picture came shrunk')
            offset, stride = self._u32(base, at + 32), self._u32(base, at + 36)
            cw, ch = self._u16(base, at + 56), self._u16(base, at + 58)
            if not cw or not ch: return self._none('the picture does not fit the window')
            full = Image.new('RGB', (cw, ch))
            if stride:                                  # an app before B8: one box around them all
                x, y, w, h = (self._u16(base, at + 40 + 2 * k) for k in range(4))
                if w and h and x + w <= cw and y + h <= ch:
                    im = Image.frombuffer('RGB', (w, h), (ctypes.c_char * (stride * h)).from_address(base + self.HEADER + offset),
                                          'raw', 'BGRX', stride, 1)
                    full.paste(im, (x, y))
            else:                                       # the parts one after another, each w * 4 bytes a row
                for k in range(len(want)):
                    x, y, w, h = (self._u16(base, ra + 8 + 16 * k + 8 + 2 * j) for j in range(4))
                    if not w or not h: continue
                    if x + w > cw or y + h > ch: return self._none('the picture does not fit the window')
                    im = Image.frombuffer('RGB', (w, h), (ctypes.c_char * (w * 4 * h)).from_address(base + self.HEADER + offset),
                                          'raw', 'BGRX', w * 4, 1)
                    full.paste(im, (x, y)); offset += w * 4 * h
            if self._u64(base, at + 16) == seq:
                self.screen_copy, self.copy_us = bool(flags & 2), self._u32(base, at + 52)
                self.source = self.SOURCES.get(flags >> 4 & 15, '?')
                self._times(base, at)
                return full
            if time.time() >= until: return self._none('the picture was rewritten while it was read')                                 # rewritten while we read it

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
        if not base or not self._request(base, int(now * 1000)): return self._none('the app shares no picture (not running, or no free reader slot)')
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
                # when the game showed it: the app writes the image's age then (window capture's own frame time; 0
                # when the source doesn't say), so fight times follow the game, not when the meter got to reading
                self.frame_time = self.picture_time - self._u16(base, at + 62) / 1000
                return im
        return None

    # Every aLogs overlay window's spot (B9), so each reader covers all of them in its own picture of the screen while
    # they stay in screenshots and recordings: 1352 + 144 * owner {u64 updated (unix ms), u32 pid, u32 count (<= 8),
    # 8 x i32 x0, y0, x1, y1 (screen px)}; owners 0 the meter, 1 Sanctum Helper, 2 the app (the button over the game).
    # A block not updated for 3 s is stale (its program closed). 1344 u64 "clean until" (unix ms): a proof is being
    # taken; every program keeps its overlays out of screen captures until then.
    CLEAN_UNTIL, OVERLAY_BASE, OVERLAY_BLOCK, OVERLAY_MAX, OVERLAY_STALE = 1344, 1352, 144, 8, 3000
    OWNER_METER, OWNER_SANCTUM, OWNER_APP = 0, 1, 2

    def publish_overlays(self, owner, rects):
        """Our overlay windows' spots (screen px: x0, y0, x1, y1) for the other readers to cover."""
        import os, time
        base = self._open(time.time())
        if not base: return
        at = self.OVERLAY_BASE + owner * self.OVERLAY_BLOCK; rects = list(rects)[:self.OVERLAY_MAX]
        for k, r in enumerate(rects):
            for j, v in enumerate(r[:4]): ctypes.c_int32.from_address(base + at + 16 + 16 * k + 4 * j).value = int(v)
        ctypes.c_uint32.from_address(base + at + 8).value = os.getpid()
        ctypes.c_uint32.from_address(base + at + 12).value = len(rects)
        ctypes.c_uint64.from_address(base + at).value = int(time.time() * 1000)

    def overlays(self, skip_owner=None):
        """Every other aLogs program's overlay spots now (screen px), to cover in our pictures."""
        import time
        base = self._open(time.time())
        if not base: return []
        now = int(time.time() * 1000); out = []
        for owner in range(3):
            if owner == skip_owner: continue
            at = self.OVERLAY_BASE + owner * self.OVERLAY_BLOCK
            if now - self._u64(base, at) > self.OVERLAY_STALE: continue
            for k in range(min(self.OVERLAY_MAX, self._u32(base, at + 12))):
                out.append(tuple(ctypes.c_int32.from_address(base + at + 16 + 16 * k + 4 * j).value for j in range(4)))
        return out

    GAME_ONLY_UNTIL = 1792
    def set_game_only_until(self, ms):
        """Asks for window capture until then (Windows 10: with its yellow border): pictures of the game alone."""
        import time
        base = self._open(time.time())
        if base: ctypes.c_uint64.from_address(base + self.GAME_ONLY_UNTIL).value = int(ms)

    def set_clean_until(self, ms):
        import time
        base = self._open(time.time())
        if base: ctypes.c_uint64.from_address(base + self.CLEAN_UNTIL).value = int(ms)

    def clean_now(self):
        import time
        base = self._open(time.time())
        return bool(base) and time.time() * 1000 < self._u64(base, self.CLEAN_UNTIL)

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
        if not base or SharedCapture._u32(base, 0) != self.MAGIC:   # not ready yet: let go, try again in 2 s
            if base: k.UnmapViewOfFile(ctypes.c_void_p(base))
            k.CloseHandle(h); return None
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

    def listening(self):
        """For the Dev switch kill-sound-log: (state, unix time audio was last heard, best match of the last few seconds,
        when it started), or None without the app. The last two only have a value with that switch on."""
        if not self.base: return None
        return (SharedCapture._u32(self.base, 32), SharedCapture._u64(self.base, 40) / 1000,
                ctypes.c_float.from_address(self.base + 44).value, SharedCapture._u64(self.base, 48) / 1000)

audio = SharedAudio()

def grab_parts(hwnd, rects):
    """Only the parts `rects` (shares 0-1) of the game's client area, the rest black (SharedCapture.read_rects)."""
    try: return shared.read_rects(hwnd, rects) if shared.app_present() else None
    except Exception: return None

def grab_window(hwnd, top_fraction=None):
    """The game's client area (or just its top part) as a PIL image, or None: the app's shared frame, None until
    the app has a fresh one (or isn't running)."""
    try: return shared.read(hwnd, top_fraction) if shared.app_present() else None
    except Exception: return None

def window_copy(hwnd):
    """The game window alone, drawn into memory by Windows (PrintWindow, ~50 ms at 4K), or None: nothing on top of it can be in
    this picture, as it can in a copy of the screen. For one picture at a time (a proof): many would make the game stutter."""
    u, g = ctypes.WinDLL('user32'), ctypes.WinDLL('gdi32')      # their own handles: these prototypes stay in here
    u.GetDC.restype = W.HDC; u.GetDC.argtypes = [W.HWND]; u.ReleaseDC.argtypes = [W.HWND, W.HDC]; u.PrintWindow.argtypes = [W.HWND, W.HDC, W.UINT]
    u.GetClientRect.argtypes = [W.HWND, ctypes.POINTER(W.RECT)]
    g.CreateCompatibleDC.restype = W.HDC; g.CreateCompatibleDC.argtypes = [W.HDC]; g.DeleteDC.argtypes = [W.HDC]
    g.CreateCompatibleBitmap.restype = W.HBITMAP; g.CreateCompatibleBitmap.argtypes = [W.HDC, ctypes.c_int, ctypes.c_int]
    g.SelectObject.restype = W.HGDIOBJ; g.SelectObject.argtypes = [W.HDC, W.HGDIOBJ]; g.DeleteObject.argtypes = [W.HGDIOBJ]
    g.GetBitmapBits.argtypes = [W.HBITMAP, ctypes.c_long, ctypes.c_void_p]
    r = W.RECT()
    if not u.GetClientRect(hwnd, ctypes.byref(r)) or r.right < 16 or r.bottom < 16: return None
    w, h = r.right, r.bottom
    screen = u.GetDC(None); mem = g.CreateCompatibleDC(screen); bmp = g.CreateCompatibleBitmap(screen, w, h); old = g.SelectObject(mem, bmp)
    try:
        if not u.PrintWindow(hwnd, mem, 3): return None         # the client area, with what the graphics card drew
        buf = ctypes.create_string_buffer(w * h * 4); g.GetBitmapBits(bmp, len(buf), buf)
        return Image.frombuffer('RGBA', (w, h), buf, 'raw', 'BGRA', 0, 1).convert('RGB')
    finally:
        g.SelectObject(mem, old); g.DeleteObject(bmp); g.DeleteDC(mem); u.ReleaseDC(None, screen)

WDA_EXCLUDEFROMCAPTURE = 0x11
def hide_from_capture(hwnd, on=True):
    """Keep our own overlay window out of screen captures (Windows 10 2004+); on=False shows it in them again."""
    return bool(user32.SetWindowDisplayAffinity(W.HWND(hwnd), WDA_EXCLUDEFROMCAPTURE if on else 0))

# While aLogs has control of an end screen, its focus guard (focusguard.py) holds the focus in the game's place so the
# game reads no controller: that window counts as the game here, and our own inputs lend the game the focus for a moment.
STANDIN = {'hwnd': None, 'guard': None}

def is_foreground(hwnd):
    fg = user32.GetForegroundWindow()
    return fg == hwnd or bool(hwnd and STANDIN['hwnd'] and fg == STANDIN['hwnd'])

class _game_moment:
    """Around one of our own clicks/keys/scrolls: the game in front for it when the focus guard holds the focus."""
    def __enter__(self):
        self.g = STANDIN['guard']
        if self.g: self.g.lend()
    def __exit__(self, *a):
        if self.g: self.g.back()

MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP = 0x2, 0x4
def click_at(x, y):
    """One left click at a screen point, then put the cursor back where it was (built-in SetCursorPos/mouse_event).
    mouse_event presses wherever the cursor is, so the cursor is put on the point again right before the press and the
    release: a player moving the mouse meanwhile can't pull the click off the button (6 Nov, it missed that way)."""
    import time
    with _game_moment():
        p = W.POINT(); user32.GetCursorPos(ctypes.byref(p))
        x, y = int(x), int(y)
        user32.SetCursorPos(x, y); time.sleep(0.12)              # the game sees the cursor arrive before the press
        user32.SetCursorPos(x, y); user32.mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0); time.sleep(0.05)
        user32.SetCursorPos(x, y); user32.mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0); time.sleep(0.03)
        user32.SetCursorPos(p.x, p.y)

KEYEVENTF_KEYUP, VK_ESCAPE = 0x2, 0x1B
def press_key(vk):
    """One key press (down, then up) to the window in front, with the key's scan code: the game reads keys by it (raw
    input), and a press without one never reached it (1 Nov: F at the Holo-Battle Sim's exit gate did nothing)."""
    import time
    with _game_moment():
        scan = user32.MapVirtualKeyW(vk, 0)                       # MAPVK_VK_TO_VSC
        user32.keybd_event(vk, scan, 0, 0); time.sleep(0.06); user32.keybd_event(vk, scan, KEYEVENTF_KEYUP, 0)

MOUSEEVENTF_WHEEL = 0x0800
def scroll_at(x, y, notches):
    """Turn the mouse wheel `notches` at a screen point (negative: down, toward the end of a list), then put the
    cursor back where it was."""
    import time
    with _game_moment():
        p = W.POINT(); user32.GetCursorPos(ctypes.byref(p))
        user32.SetCursorPos(int(x), int(y)); time.sleep(0.03)
        for _ in range(abs(int(notches))):
            user32.mouse_event(MOUSEEVENTF_WHEEL, 0, 0, 120 if notches > 0 else -120, 0); time.sleep(0.03)
        user32.SetCursorPos(p.x, p.y)
