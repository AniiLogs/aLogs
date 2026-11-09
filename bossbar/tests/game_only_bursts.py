"""Result, Party and menu screens are read from the game window alone, in bursts of window capture.

    python tests/game_only_bursts.py

Meter.game_only / game_only_gone (8 Nov: a copy of the screen shows a picture put over the game):
- one of these screens asks for window capture and keeps asking while it's up, without a write on every frame;
- on a copy of the screen it waits for the window capture, then reads what it has if that never comes;
- from the game window it reads straight away;
- other screens neither ask nor wait;
- a screen spotted on the copy that the game window doesn't show is said in the log.
The shared memory is stubbed. Exit code 1 on any failure.
"""
import os, sys, tempfile, logging
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_burst_')
import live
from live import Meter

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

class Shared:
    screen_copy = True; asks = 0
    def set_game_only_until(self, ms): Shared.asks += 1
keep = live.wc.shared; live.wc.shared = Shared()
said = []
class Catch(logging.Handler):
    def emit(self, record): said.append(record.getMessage())
live.log.addHandler(Catch()); live.log.setLevel(logging.INFO)
try:
    m = Meter.__new__(Meter)
    check('another screen: no wait', m.game_only('Catch Countdown', 100.0), False)
    check('...and no ask', Shared.asks, 0)
    check('nothing spotted: no wait', m.game_only(None, 100.0), False)
    for screen in ('result', 'party', 'menu', 'Battle Data', 'Egg Heist'):
        n = Meter.__new__(Meter); Shared.asks = 0
        check(f'{screen}: waits on a copy of the screen', n.game_only(screen, 100.0), True)
        check(f'{screen}: asks', Shared.asks, 1)
    m = Meter.__new__(Meter); Shared.asks = 0
    m.game_only('result', 100.0); m.game_only('result', 100.1); m.game_only('result', 100.2)
    check('not a write on every frame', Shared.asks, 1)
    m.game_only('result', 100.7); check('...but it keeps asking', Shared.asks, 2)
    check('still within the wait', m.game_only('result', 101.0), True)
    check('it never opened: reads the screen', m.game_only('result', 101.6), False)
    check('...and says so', any('did not open' in s for s in said), True)
    check('a new screen later waits again', m.game_only('result', 110.0), True)

    Shared.screen_copy = False
    m = Meter.__new__(Meter)
    check('from the game window: reads at once', m.game_only('party', 200.0), False)
    check('...and still asks, to keep it open', Shared.asks > 0, True)

    said.clear(); Shared.screen_copy = True
    m = Meter.__new__(Meter); m.game_only('result', 300.0)
    Shared.screen_copy = False
    m.game_only_gone('result', 300.5); check('the game window shows it too: nothing to say', said, [])
    m.game_only('result', 300.6); m.game_only_gone(None, 300.7); check('left as it should: nothing to say', said, [])
    Shared.screen_copy = True
    m = Meter.__new__(Meter); m.game_only('result', 400.0)
    m.game_only_gone(None, 400.2); check('still on the copy: nothing known yet', said, [])
    Shared.screen_copy = False
    m.game_only_gone(None, 400.6); check('the game window does not show it: said', len([s for s in said if "doesn't" in s]), 1)
    m.game_only_gone(None, 400.8); check('...once', len([s for s in said if "doesn't" in s]), 1)
finally:
    live.wc.shared = keep
if fails:
    print('\n'.join(fails)); sys.exit(1)
print('game only bursts: ok')
