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
