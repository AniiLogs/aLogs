"""Hides game controllers from the game for a moment, through HidHide's driver (Nefarius, MIT).

The desktop app installs the driver itself (Results > Controller; one admin approval) and puts your gamepads on its
block list. During the automatic "View Battle Data" moment the meter turns hiding on, and off again right after, by
talking to the driver directly (no HidHide program needed). Without the driver every call here does nothing.
"""
import ctypes, logging
from ctypes import wintypes

log = logging.getLogger('meter')
DEVICE = r'\\.\HidHide'
IOCTL_SET_ACTIVE = (32769 << 16) | (1 << 14) | (2053 << 2)   # HidHide's Shared/HidHideIoctlContract.h
hidden = False

_k32 = ctypes.WinDLL('kernel32', use_last_error=True)
_k32.CreateFileW.restype = wintypes.HANDLE
_k32.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, ctypes.c_void_p, wintypes.DWORD,
                             wintypes.DWORD, wintypes.HANDLE]
_k32.DeviceIoControl.argtypes = [wintypes.HANDLE, wintypes.DWORD, ctypes.c_void_p, wintypes.DWORD, ctypes.c_void_p,
                                 wintypes.DWORD, ctypes.POINTER(wintypes.DWORD), ctypes.c_void_p]
_k32.CloseHandle.argtypes = [wintypes.HANDLE]
INVALID = wintypes.HANDLE(-1).value

def _open():
    h = _k32.CreateFileW(DEVICE, 0x80000000, 3, None, 3, 0, None)   # GENERIC_READ, share read/write, OPEN_EXISTING
    return None if h in (None, INVALID) else h

def available():
    h = _open()
    if h: _k32.CloseHandle(h)
    return bool(h)

def hide_controllers(on):
    """Turn hiding on (controllers on the block list disappear for apps) or off."""
    global hidden
    h = _open()
    if not h: return False
    try:
        flag, got = ctypes.c_ubyte(1 if on else 0), wintypes.DWORD()
        ok = bool(_k32.DeviceIoControl(h, IOCTL_SET_ACTIVE, ctypes.byref(flag), 1, None, 0, ctypes.byref(got), None))
    finally:
        _k32.CloseHandle(h)
    hidden = on and ok
    log.info(f"controller {'hidden' if on else 'visible again'}" + ('' if ok else f' - HidHide error {ctypes.get_last_error()}'))
    return ok

# ---- Reading presses (XInput: any app may read a controller, in front or not; with reWASD it's reWASD's virtual pad,
# the one the game reads). Nothing is sent or blocked here.
class _PAD(ctypes.Structure):
    _fields_ = [('wButtons', wintypes.WORD), ('bLeftTrigger', ctypes.c_ubyte), ('bRightTrigger', ctypes.c_ubyte),
                ('sThumbLX', ctypes.c_short), ('sThumbLY', ctypes.c_short), ('sThumbRX', ctypes.c_short), ('sThumbRY', ctypes.c_short)]
class _STATE(ctypes.Structure):
    _fields_ = [('dwPacketNumber', wintypes.DWORD), ('Gamepad', _PAD)]
_xinput = None
for _name in ('xinput1_4', 'xinput1_3', 'xinput9_1_0'):
    try: _xinput = ctypes.WinDLL(_name); break
    except OSError: pass
XINPUT_Y = 0x8000
_u32 = ctypes.windll.user32

def pad_buttons():
    """The buttons held on any connected controller (XInput bits), 0 without one."""
    if not _xinput: return 0
    held, st = 0, _STATE()
    for i in range(4):
        if _xinput.XInputGetState(i, ctypes.byref(st)) == 0: held |= st.Gamepad.wButtons
    return held

class PressWatch:
    """Watches, 100 times a second, for presses of the controller's Y or the keyboard's F (each the game's interact
    button) while on: `last` is the newest one, (what, when). Reads only; the press still goes to the game."""
    def __init__(self): self.last = None; self.on = False
    def start(self):
        if self.on: return
        import threading
        self.on = True
        threading.Thread(target=self._run, daemon=True).start()
    def stop(self): self.on = False
    def _run(self):
        import time
        was_y = bool(pad_buttons() & XINPUT_Y); was_f = bool(_u32.GetAsyncKeyState(0x46) & 0x8000)
        while self.on:
            y = bool(pad_buttons() & XINPUT_Y); f = bool(_u32.GetAsyncKeyState(0x46) & 0x8000)
            if y and not was_y: self.last = ('Y', time.time())
            elif f and not was_f: self.last = ('F', time.time())
            was_y, was_f = y, f
            time.sleep(0.01)
