"""Keeps the controller away from the game while aLogs has control of an end screen (built-in Windows, no admin).

An invisible window of aLogs' own takes the focus from the game. The game reads no controller while it isn't the
active window (checked by the owner, 12 Nov), and the input hold (inputblock.py) swallows keys and mouse buttons
meanwhile, so nothing of the player's reaches it. HidHide can't do this: it only stops a program from opening a
controller, and the game opened it when it started (11 Nov: a Holo-Battle Sim end "hidden" was skipped with the
controller 1.4 s after the Victory screen came up).

- The game keeps drawing in the background, so its screens are still read (the app's tabbed-out copy, 4 a second).
- Our own clicks, keys and scrolls (wincapture.click_at etc.) give the game the focus for that moment (`lend`),
  and take it back after.
- wincapture.is_foreground counts this window as the game, so nothing else takes it for "you tabbed out".
- Hard limit: the window gives the focus back and closes itself after `seconds`, whatever happens.
"""
import ctypes, ctypes.wintypes as W, threading, time

user32 = ctypes.windll.user32; kernel32 = ctypes.windll.kernel32
WNDPROC = ctypes.WINFUNCTYPE(ctypes.c_ssize_t, W.HWND, W.UINT, W.WPARAM, W.LPARAM)

class WNDCLASSW(ctypes.Structure):
    _fields_ = [('style', W.UINT), ('lpfnWndProc', WNDPROC), ('cbClsExtra', ctypes.c_int), ('cbWndExtra', ctypes.c_int),
                ('hInstance', W.HINSTANCE), ('hIcon', W.HANDLE), ('hCursor', W.HANDLE), ('hbrBackground', W.HANDLE),
                ('lpszMenuName', W.LPCWSTR), ('lpszClassName', W.LPCWSTR)]

user32.DefWindowProcW.argtypes = [W.HWND, W.UINT, W.WPARAM, W.LPARAM]; user32.DefWindowProcW.restype = ctypes.c_ssize_t
user32.RegisterClassW.argtypes = [ctypes.POINTER(WNDCLASSW)]; user32.RegisterClassW.restype = W.ATOM
user32.CreateWindowExW.argtypes = [W.DWORD, W.LPCWSTR, W.LPCWSTR, W.DWORD, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                                   ctypes.c_int, W.HWND, W.HMENU, W.HINSTANCE, W.LPVOID]
user32.CreateWindowExW.restype = W.HWND
user32.GetForegroundWindow.restype = W.HWND
user32.GetWindowThreadProcessId.argtypes = [W.HWND, ctypes.POINTER(W.DWORD)]; user32.GetWindowThreadProcessId.restype = W.DWORD
user32.SetForegroundWindow.argtypes = [W.HWND]; user32.BringWindowToTop.argtypes = [W.HWND]
user32.ShowWindow.argtypes = [W.HWND, ctypes.c_int]; user32.DestroyWindow.argtypes = [W.HWND]
user32.SetLayeredWindowAttributes.argtypes = [W.HWND, W.DWORD, ctypes.c_ubyte, W.DWORD]
user32.PostMessageW.argtypes = [W.HWND, W.UINT, W.WPARAM, W.LPARAM]
user32.MsgWaitForMultipleObjects.argtypes = [W.DWORD, ctypes.c_void_p, W.BOOL, W.DWORD, W.DWORD]
user32.PeekMessageW.argtypes = [ctypes.POINTER(W.MSG), W.HWND, W.UINT, W.UINT, W.UINT]
kernel32.GetModuleHandleW.argtypes = [W.LPCWSTR]; kernel32.GetModuleHandleW.restype = W.HMODULE

WS_POPUP = 0x80000000
WS_EX_LAYERED, WS_EX_TRANSPARENT, WS_EX_TOOLWINDOW, WS_EX_TOPMOST = 0x80000, 0x20, 0x80, 0x8
LWA_ALPHA, SW_SHOW, PM_REMOVE, QS_ALLINPUT = 0x2, 5, 0x1, 0x04FF
CLASS = 'aLogsFocusGuard'
_class_done = [False]
_wndproc = WNDPROC(lambda h, m, w, l: user32.DefWindowProcW(h, m, w, l))    # kept alive for the class's life

def bring_to_front(hwnd, tries=3):
    """Make `hwnd` the active window. Windows lets a background program do that only in some cases, so this joins
    the input of the window now in front for the moment of the call (the usual way), then checks."""
    for _ in range(tries):
        fg = user32.GetForegroundWindow()
        if fg == hwnd: return True
        me = kernel32.GetCurrentThreadId(); other = user32.GetWindowThreadProcessId(fg, None) if fg else 0
        joined = bool(other and other != me and user32.AttachThreadInput(me, other, True))
        try:
            user32.BringWindowToTop(hwnd); user32.SetForegroundWindow(hwnd)
        finally:
            if joined: user32.AttachThreadInput(me, other, False)
        if user32.GetForegroundWindow() == hwnd: return True
        time.sleep(0.02)
    return user32.GetForegroundWindow() == hwnd

class FocusGuard:
    LEND_SETTLE = 0.15    # s the game has the focus before our click: it takes a few frames to read input again
    LEND_AFTER = 0.12     # s it keeps it after: the click's release and the game's reaction to it land first (14 Nov)

    def __init__(self, log=None):
        self.log = log or (lambda m: None)
        self.hwnd = None; self.game = None; self.stop_at = 0.0; self.thread = None; self.lent = 0
        self.lock = threading.RLock(); self.ready = threading.Event()

    @property
    def active(self): return self.thread is not None and self.thread.is_alive() and bool(self.hwnd)

    def take(self, game_hwnd, seconds=60):
        """Take the focus from `game_hwnd` for at most `seconds` (again from now when it's already on). False when
        Windows wouldn't give it (then nothing is changed: the game keeps the focus)."""
        if self.active:
            self.stop_at = time.time() + seconds; return True
        self.game = game_hwnd; self.stop_at = time.time() + seconds; self.ready.clear(); self.hwnd = None
        self.thread = threading.Thread(target=self._run, daemon=True); self.thread.start()
        if not self.ready.wait(2.0) or not self.hwnd:
            self.log('focus guard: its window could not be made'); self.stop_at = 0.0; return False
        import wincapture as wc
        with self.lock:
            ok = bring_to_front(self.hwnd)
            if ok: wc.STANDIN['hwnd'] = self.hwnd; wc.STANDIN['guard'] = self
        if not ok:
            self.log('focus guard: Windows did not give it the focus; the controller still reaches the game')
            self.stop_at = 0.0
            return False
        self.log('focus guard on: the game is in the background, so it reads no controller')
        return True

    def stop(self): self.stop_at = 0.0

    def lend(self):
        """The game in front for one of our own clicks; `back` takes the focus again."""
        with self.lock:
            if not self.active: return
            self.lent += 1
            if self.lent == 1 and bring_to_front(self.game): time.sleep(self.LEND_SETTLE)

    def back(self):
        with self.lock:
            if not self.lent: return
            self.lent -= 1
            if not self.lent and self.active and time.time() < self.stop_at:
                time.sleep(self.LEND_AFTER); bring_to_front(self.hwnd)

    def _run(self):
        import wincapture as wc
        mod = kernel32.GetModuleHandleW(None)
        if not _class_done[0]:
            wc_ = WNDCLASSW(); wc_.lpfnWndProc = _wndproc; wc_.hInstance = mod; wc_.lpszClassName = CLASS
            _class_done[0] = bool(user32.RegisterClassW(ctypes.byref(wc_))) or bool(kernel32.GetLastError() == 1410)  # already registered
        # one pixel, fully see-through and click-through, over the game's top-left corner, kept out of the taskbar
        r = W.RECT(); user32.GetWindowRect(W.HWND(self.game), ctypes.byref(r))
        h = user32.CreateWindowExW(WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST, CLASS, 'aLogs',
                                   WS_POPUP, r.left, r.top, 1, 1, None, None, mod, None)
        if h:
            user32.SetLayeredWindowAttributes(h, 0, 0, LWA_ALPHA)
            try: wc.hide_from_capture(h)
            except Exception: pass
            user32.ShowWindow(h, SW_SHOW)
        self.hwnd = h; self.ready.set()
        if not h: return
        msg = W.MSG()
        try:
            while time.time() < self.stop_at:                         # the window answers Windows while it's up
                user32.MsgWaitForMultipleObjects(0, None, False, 20, QS_ALLINPUT)
                while user32.PeekMessageW(ctypes.byref(msg), None, 0, 0, PM_REMOVE):
                    user32.TranslateMessage(ctypes.byref(msg)); user32.DispatchMessageW(ctypes.byref(msg))
        finally:
            with self.lock:
                wc.STANDIN['hwnd'] = None; wc.STANDIN['guard'] = None
                was_front = user32.GetForegroundWindow() == h
                if was_front: bring_to_front(self.game)               # the game back in front, as it was
                user32.DestroyWindow(h); self.hwnd = None; self.lent = 0
            self.log('focus guard off: the game has the focus again' if was_front else 'focus guard off')
