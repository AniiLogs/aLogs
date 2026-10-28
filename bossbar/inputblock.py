"""Briefly holds back keyboard keys and mouse buttons (built-in Windows low-level hooks, no admin, nothing installed).

Used on the boss rush result screen so button-mashing can't leave it before 'View Battle Data' is opened.
- Mouse movement still works; only keys, mouse buttons and the wheel are swallowed.
- Our own clicks (marked by Windows as injected) pass through.
- Hard limit: the hook removes itself after `seconds` no matter what (its own thread watches the clock).
- Ctrl+Shift+F12 releases it at once; Ctrl+Alt+Del always works (Windows never lets hooks see it).
- Controllers are not affected (that would need a driver).
"""
import ctypes, ctypes.wintypes as W, threading, time

user32 = ctypes.windll.user32; kernel32 = ctypes.windll.kernel32
WH_KEYBOARD_LL, WH_MOUSE_LL = 13, 14
WM_MOUSEMOVE, WM_QUIT = 0x0200, 0x0012
LLKHF_INJECTED, LLMHF_INJECTED = 0x10, 0x1
VK_SHIFT, VK_CONTROL, VK_F12 = 0x10, 0x11, 0x7B
PM_REMOVE, QS_ALLINPUT = 0x1, 0x04FF

class KBD(ctypes.Structure):
    _fields_ = [('vkCode', W.DWORD), ('scanCode', W.DWORD), ('flags', W.DWORD), ('time', W.DWORD), ('extra', ctypes.c_size_t)]
class MSE(ctypes.Structure):
    _fields_ = [('pt', W.POINT), ('mouseData', W.DWORD), ('flags', W.DWORD), ('time', W.DWORD), ('extra', ctypes.c_size_t)]

HOOKPROC = ctypes.WINFUNCTYPE(ctypes.c_ssize_t, ctypes.c_int, W.WPARAM, W.LPARAM)
user32.SetWindowsHookExW.argtypes = [ctypes.c_int, HOOKPROC, W.HINSTANCE, W.DWORD]
user32.SetWindowsHookExW.restype = W.HHOOK
user32.CallNextHookEx.argtypes = [W.HHOOK, ctypes.c_int, W.WPARAM, W.LPARAM]
user32.CallNextHookEx.restype = ctypes.c_ssize_t
user32.UnhookWindowsHookEx.argtypes = [W.HHOOK]
kernel32.GetModuleHandleW.argtypes = [W.LPCWSTR]; kernel32.GetModuleHandleW.restype = W.HMODULE   # 64-bit handle
user32.MsgWaitForMultipleObjects.argtypes = [W.DWORD, ctypes.c_void_p, W.BOOL, W.DWORD, W.DWORD]
user32.PeekMessageW.argtypes = [ctypes.POINTER(W.MSG), W.HWND, W.UINT, W.UINT, W.UINT]

class InputBlock:
    def __init__(self, log=None):
        self.log = log or (lambda m: None); self.thread = None; self.stop_at = 0.0; self.swallowed = 0

    @property
    def active(self): return self.thread is not None and self.thread.is_alive()

    def start(self, seconds=60):
        if self.active: return
        self.stop_at = time.time() + seconds; self.swallowed = 0
        self.thread = threading.Thread(target=self._run, daemon=True); self.thread.start()

    def stop(self): self.stop_at = 0.0

    # releases always go through: a key or button held when the hold began would otherwise stay down in the game
    KEY_UPS = (0x101, 0x105)                       # WM_KEYUP, WM_SYSKEYUP
    BUTTON_UPS = (0x202, 0x205, 0x208, 0x20C)      # left, right, middle, X button up

    def _run(self):
        def kb(code, wp, lp):
            if code == 0 and wp not in self.KEY_UPS:
                k = ctypes.cast(lp, ctypes.POINTER(KBD)).contents
                if k.vkCode == VK_F12 and user32.GetAsyncKeyState(VK_CONTROL) & 0x8000 and user32.GetAsyncKeyState(VK_SHIFT) & 0x8000:
                    self.stop_at = 0.0; return 1
                if not k.flags & LLKHF_INJECTED and k.vkCode not in (VK_SHIFT, VK_CONTROL, 0xA0, 0xA1, 0xA2, 0xA3):
                    self.swallowed += 1; return 1
            return user32.CallNextHookEx(None, code, wp, lp)
        def ms(code, wp, lp):
            if code == 0 and wp != WM_MOUSEMOVE and wp not in self.BUTTON_UPS:
                m = ctypes.cast(lp, ctypes.POINTER(MSE)).contents
                if not m.flags & LLMHF_INJECTED: self.swallowed += 1; return 1
            return user32.CallNextHookEx(None, code, wp, lp)
        kb_proc, ms_proc = HOOKPROC(kb), HOOKPROC(ms)                     # keep references alive
        mod = kernel32.GetModuleHandleW(None)
        hk = user32.SetWindowsHookExW(WH_KEYBOARD_LL, kb_proc, mod, 0)
        hm = user32.SetWindowsHookExW(WH_MOUSE_LL, ms_proc, mod, 0)
        err = ctypes.GetLastError() if not (hk and hm) else 0
        self.log(f'input hold on (keyboard hook {"ok" if hk else "FAILED"}, mouse hook {"ok" if hm else "FAILED"}'
                 + (f', error {err}' if err else '') + ')')
        msg = W.MSG()
        try:
            while time.time() < self.stop_at:                            # the hooks only run while this thread pumps
                user32.MsgWaitForMultipleObjects(0, None, False, 20, QS_ALLINPUT)
                while user32.PeekMessageW(ctypes.byref(msg), None, 0, 0, PM_REMOVE): pass
        finally:
            if hk: user32.UnhookWindowsHookEx(hk)
            if hm: user32.UnhookWindowsHookEx(hm)
            self.log(f'input hold off ({self.swallowed} presses held back)')
