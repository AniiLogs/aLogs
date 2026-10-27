"""Per-pixel alpha overlay windows (Windows layered windows, UpdateLayeredWindow).

Tk's own transparency is one colour key: a pixel is either fully see-through or solid, so soft text shadows and
anti-aliased edges turn into a hard dark halo. Here the whole window is one premultiplied RGBA bitmap instead, so the
overlay text blends into the game like the game's own text. Fully transparent pixels also let clicks through.
"""
import ctypes, ctypes.wintypes as W
from PIL import Image, ImageChops

user32 = ctypes.windll.user32; gdi32 = ctypes.windll.gdi32
GWL_EXSTYLE, WS_EX_LAYERED, ULW_ALPHA, AC_SRC_ALPHA = -20, 0x80000, 2, 1

class BLENDFUNCTION(ctypes.Structure):
    _fields_ = [('BlendOp', ctypes.c_ubyte), ('BlendFlags', ctypes.c_ubyte),
                ('SourceConstantAlpha', ctypes.c_ubyte), ('AlphaFormat', ctypes.c_ubyte)]

class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [('biSize', W.DWORD), ('biWidth', W.LONG), ('biHeight', W.LONG), ('biPlanes', W.WORD),
                ('biBitCount', W.WORD), ('biCompression', W.DWORD), ('biSizeImage', W.DWORD),
                ('biXPelsPerMeter', W.LONG), ('biYPelsPerMeter', W.LONG), ('biClrUsed', W.DWORD), ('biClrImportant', W.DWORD)]

user32.GetWindowLongW.restype = ctypes.c_long
user32.GetWindowLongW.argtypes = [W.HWND, ctypes.c_int]
user32.SetWindowLongW.argtypes = [W.HWND, ctypes.c_int, ctypes.c_long]
user32.UpdateLayeredWindow.argtypes = [W.HWND, W.HDC, ctypes.POINTER(W.POINT), ctypes.POINTER(W.SIZE), W.HDC,
                                       ctypes.POINTER(W.POINT), W.DWORD, ctypes.POINTER(BLENDFUNCTION), W.DWORD]
gdi32.CreateDIBSection.restype = W.HBITMAP
gdi32.CreateDIBSection.argtypes = [W.HDC, ctypes.c_void_p, W.UINT, ctypes.POINTER(ctypes.c_void_p), W.HANDLE, W.DWORD]
gdi32.CreateCompatibleDC.restype = W.HDC
gdi32.SelectObject.restype = W.HGDIOBJ
gdi32.SelectObject.argtypes = [W.HDC, W.HGDIOBJ]
gdi32.DeleteObject.argtypes = [W.HGDIOBJ]
gdi32.DeleteDC.argtypes = [W.HDC]
user32.GetDC.restype = W.HDC
user32.ReleaseDC.argtypes = [W.HWND, W.HDC]

_ready = set()

def top_hwnd(tk_window):
    """The real top-level window behind a Tk Toplevel."""
    return user32.GetParent(tk_window.winfo_id()) or tk_window.winfo_id()

def show(hwnd, image, x, y):
    """Shows an RGBA PIL image as the whole window at screen (x, y)."""
    if hwnd not in _ready:
        # restart the layered style, in case a colour key was set on it before (that mode can't take a bitmap)
        style = user32.GetWindowLongW(hwnd, GWL_EXSTYLE)
        user32.SetWindowLongW(hwnd, GWL_EXSTYLE, style & ~WS_EX_LAYERED)
        user32.SetWindowLongW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED)
        _ready.add(hwnd)
    image = image.convert('RGBA'); w, h = image.size
    r, g, b, a = image.split()
    data = Image.merge('RGBA', (ImageChops.multiply(b, a), ImageChops.multiply(g, a), ImageChops.multiply(r, a), a)).tobytes()
    screen = user32.GetDC(None); mem = gdi32.CreateCompatibleDC(screen)
    bmi = BITMAPINFOHEADER(ctypes.sizeof(BITMAPINFOHEADER), w, -h, 1, 32, 0, 0, 0, 0, 0, 0)
    bits = ctypes.c_void_p()
    bmp = gdi32.CreateDIBSection(screen, ctypes.byref(bmi), 0, ctypes.byref(bits), None, 0)
    try:
        if not bmp or not bits: return False
        ctypes.memmove(bits, data, len(data))
        old = gdi32.SelectObject(mem, bmp)
        blend = BLENDFUNCTION(0, 0, 255, AC_SRC_ALPHA)
        ok = user32.UpdateLayeredWindow(hwnd, screen, ctypes.byref(W.POINT(int(x), int(y))), ctypes.byref(W.SIZE(w, h)),
                                        mem, ctypes.byref(W.POINT(0, 0)), 0, ctypes.byref(blend), ULW_ALPHA)
        gdi32.SelectObject(mem, old)
        return bool(ok)
    finally:
        if bmp: gdi32.DeleteObject(bmp)
        gdi32.DeleteDC(mem); user32.ReleaseDC(None, screen)

def set_alpha(hwnd, alpha):
    """Fades the whole window (0-255) without sending its bitmap again: cheap enough to animate every frame."""
    blend = BLENDFUNCTION(0, 0, max(0, min(255, int(alpha))), AC_SRC_ALPHA)
    return bool(user32.UpdateLayeredWindow(hwnd, None, None, None, None, None, 0, ctypes.byref(blend), ULW_ALPHA))

WS_EX_TRANSPARENT, WS_EX_NOACTIVATE = 0x20, 0x08000000
def click_through(hwnd):
    """Clicks go to whatever is under the window, even where it is drawn (a full-screen effect over other apps)."""
    user32.SetWindowLongW(hwnd, GWL_EXSTYLE, user32.GetWindowLongW(hwnd, GWL_EXSTYLE) | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE)
