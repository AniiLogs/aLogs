"""The focus guard's bookkeeping, without taking the real focus (made-up windows, nothing on screen):
- wincapture.is_foreground counts the guard's window as the game while it stands in;
- our own clicks lend the game the focus and take it back (once, even when nested);
- the Holo-Battle Sim's Gizmo tour stops once the game has left the Sim (11 Nov: it went on clicking in the next place).
"""
import os, sys
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import wincapture as wc

calls = []
class FakeGuard:
    def lend(self): calls.append('lend')
    def back(self): calls.append('back')

real_fg = wc.user32.GetForegroundWindow
try:
    wc.user32.GetForegroundWindow = lambda: 222
    assert not wc.is_foreground(111)
    wc.STANDIN.update(hwnd=222, guard=FakeGuard())
    assert wc.is_foreground(111), 'the guard window stands in for the game'
    assert not wc.is_foreground(None)
    with wc._game_moment(): pass
    assert calls == ['lend', 'back'], calls
finally:
    wc.user32.GetForegroundWindow = real_fg
    wc.STANDIN.update(hwnd=None, guard=None)
with wc._game_moment(): pass
assert calls == ['lend', 'back'], 'no guard: nothing lent'

# the guard's own counting: nested lends bring the game forward once and the guard back once
import focusguard as fgmod
moves = []
fgmod.bring_to_front = lambda h, tries=3: moves.append(h) or True
g = fgmod.FocusGuard(); g.game = 111; g.hwnd = 222; g.stop_at = 1e18
g.thread = type('T', (), {'is_alive': lambda self: True})()
g.LEND_SETTLE = 0
g.lend(); g.lend(); g.back(); g.back()
assert moves == [111, 222], moves

# the tour ends once the game left the Sim
import leaderboards as L
ended = []
class M:
    sim_tour = {'stage': 'open', 'next': 0}
    sim_ctl = {'until': 1e18, 'held': False, 'space': 'A'}
    space = 'B'; hwnd = 1
    def sim_tour_end(self, why): ended.append(why)
wc.STANDIN.update(hwnd=None, guard=None)
real_is_fg = L.wc.is_foreground
L.wc.is_foreground = lambda h: True
try:
    assert L.LeaderboardsMixin.sim_tour_step(M(), None, 5.0) is False
    assert ended == ['the game left the Victory screen'], ended
finally:
    L.wc.is_foreground = real_is_fg
print('focus guard: ok')
