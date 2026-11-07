"""The automatic Battle Data click lands where it's aimed, and a finished run is sent on time.

    python tests/auto_click.py

- wincapture.click_at puts the cursor on the button again right before the press and the release (mouse_event
  presses wherever the cursor is: a player moving the mouse pulled the click off the button, 6 Nov), then puts it back.
- InputBlock.freeze_moves holds back the player's mouse movement for a moment around our click, only then.
- ResultsMixin.schedule_run: a run is decided BATTLE_DATA_SETTLE after its Battle Data is in, BATTLE_DATA_WAIT after
  its result without one; later reads of either screen never push that back (it waited until the player left the
  screens, 20-30 s after the result, and its "sent" notice with it).
- screen_read_due: while the automatic session waits for its first click, the result screen is read 4 times a
  second, not every 1.5 s (the click came ~4 s after the screen); once it has clicked, the usual pace.
- picture_gap: pictures stopping for 2 s or more is logged with the capture's reason, and so is their return.
Stand-ins only (no game, no real clicks: user32 is replaced). Exit code 1 on any failure.
"""
import os, sys, tempfile, time
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_click_')
import wincapture as wc, inputblock, live, results

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

# -- click_at: the cursor is on the button at the press and the release ---------------------------------------------
class FakeUser32:
    def __init__(self): self.calls = []; self.pos = (100, 100)
    def GetCursorPos(self, ref): ref._obj.x, ref._obj.y = self.pos
    def SetCursorPos(self, x, y): self.calls.append(('at', x, y)); self.pos = (x, y)
    def mouse_event(self, flags, *a):
        self.calls.append(('down' if flags == wc.MOUSEEVENTF_LEFTDOWN else 'up', *self.pos))
        self.pos = (self.pos[0] + 37, self.pos[1] + 21)          # the player moves the mouse after every event
real, fake = wc.user32, FakeUser32()
wc.user32 = fake
try: wc.click_at(2304, 1944)
finally: wc.user32 = real
presses = [c for c in fake.calls if c[0] in ('down', 'up')]
check('press and release on the button', presses, [('down', 2304, 1944), ('up', 2304, 1944)])
check('cursor put back where it was', fake.calls[-1], ('at', 100, 100))

# -- freeze_moves: only for a moment ----------------------------------------------------------------------------------
hold = inputblock.InputBlock()
check('moves not held back by default', hold.moves_until <= time.time(), True)
hold.freeze_moves(0.4)
check('moves held back around a click', hold.moves_until > time.time(), True)
check('...for under half a second', hold.moves_until - time.time() <= 0.41, True)

# -- schedule_run: the deadline only moves earlier --------------------------------------------------------------------
m = live.Meter.__new__(live.Meter); m.pending_run = None
m.schedule_run('run.json', False, 1000)
check('result alone: waits for Battle Data', m.pending_run, ('run.json', 1000 + results.BATTLE_DATA_WAIT))
m.schedule_run('run.json', True, 1010)
check('Battle Data in: settles', m.pending_run, ('run.json', 1010 + results.BATTLE_DATA_SETTLE))
for t in (1012, 1014, 1020, 1030):                               # more reads of both screens
    m.schedule_run('run.json', True, t)
check('later reads keep the deadline', m.pending_run, ('run.json', 1010 + results.BATTLE_DATA_SETTLE))
m.schedule_run('next.json', False, 1200)
check('a new run gets its own', m.pending_run, ('next.json', 1200 + results.BATTLE_DATA_WAIT))

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('auto click: ok')
