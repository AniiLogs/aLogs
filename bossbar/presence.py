"""Online / AFK for the player's AniiLogs profile, while the game runs and aLogs is on.

You're AFK when either has gone on for 20 minutes:
- Aniimo isn't the window in front (you're in another window or tabbed out), or
- nobody has touched the PC: no keyboard or mouse input (Windows' own last-input time) and no controller input (any
  XInput pad's state changing: a button, a stick, a trigger).
Anything else is online. The game's own log says nothing about idling, so this is all it goes on.

Nothing here sends anything: Meter.presence_tick asks for the state about once a second and the submitter sends it.
"""
import ctypes, time
from ctypes import wintypes

AFK_AFTER = 20 * 60          # seconds

class _LastInput(ctypes.Structure):
    _fields_ = [('cbSize', wintypes.UINT), ('dwTime', wintypes.DWORD)]

class _Gamepad(ctypes.Structure):
    _fields_ = [('wButtons', wintypes.WORD), ('bLeftTrigger', ctypes.c_ubyte), ('bRightTrigger', ctypes.c_ubyte),
                ('sThumbLX', ctypes.c_short), ('sThumbLY', ctypes.c_short), ('sThumbRX', ctypes.c_short), ('sThumbRY', ctypes.c_short)]

class _XState(ctypes.Structure):
    _fields_ = [('dwPacketNumber', wintypes.DWORD), ('Gamepad', _Gamepad)]

def _xinput():
    for name in ('xinput1_4', 'xinput1_3', 'xinput9_1_0'):
        try: return getattr(ctypes.windll, name).XInputGetState
        except OSError: continue
    return None

def keyboard_mouse_idle():
    """Seconds since the last keyboard or mouse input anywhere on this PC."""
    li = _LastInput(ctypes.sizeof(_LastInput), 0)
    if not ctypes.windll.user32.GetLastInputInfo(ctypes.byref(li)): return 0.0
    return ((ctypes.windll.kernel32.GetTickCount() - li.dwTime) & 0xFFFFFFFF) / 1000.0


class Presence:
    def __init__(self):
        self.get_state = _xinput()
        self.pads = {}                   # pad index -> its last packet number (it changes whenever the pad's state does)
        self.pad_input = time.time()
        self.in_front = time.time()
        self.pad_check = 0.0

    def controller_idle(self, now):
        if self.get_state and now - self.pad_check >= 1:
            self.pad_check = now
            for i in range(4):
                st = _XState()
                if self.get_state(i, ctypes.byref(st)) != 0: self.pads.pop(i, None); continue   # no pad there
                if self.pads.get(i) not in (None, st.dwPacketNumber): self.pad_input = now
                self.pads[i] = st.dwPacketNumber
        return now - self.pad_input

    def state(self, now, game_in_front):
        """'online' or 'afk'."""
        if game_in_front: self.in_front = now
        idle = min(keyboard_mouse_idle(), self.controller_idle(now))
        return 'afk' if now - self.in_front >= AFK_AFTER or idle >= AFK_AFTER else 'online'
