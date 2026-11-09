"""A proof is of the game, not of what's on top of it: the screen against the game window alone.

    python tests/screen_check.py

results.screen_difference and Meter.game_alone (8 Nov: a copy of the screen shows a picture put over the game):
- the same picture differs by nothing; a panel over the middle shows up; a counter in the corner doesn't count;
- the game window's copy becomes the proof, and what it found goes with it;
- a copy that's black, another size or missing leaves the picture read, and says so;
- a copy nothing like the picture read (the game moved on) leaves the picture read;
- window capture needs no copy: it only ever sees the game;
- the check reaches a run's payload and an Egg Heist match's.
Made-up pictures; the copy is stubbed. Exit code 1 on any failure.
"""
import os, sys, tempfile
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_check_')
from PIL import Image, ImageDraw
import results, submit
import wincapture as wc
from live import Meter
import logging; logging.disable(logging.CRITICAL)

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

W, H = 1920, 1080
GAME = Image.new('RGB', (W, H), (90, 110, 130))
def with_box(box, colour=(240, 240, 240)):
    im = GAME.copy(); ImageDraw.Draw(im).rectangle(box, fill=colour); return im
d = results.screen_difference
check('the same picture', d(GAME, GAME.copy())['worst'], 0)
panel = d(with_box((700, 400, 1200, 700)), GAME)
check('a panel on top shows', panel['worst'] > 100 and panel['over40'] > 10, True)
check('a small patch over a number shows', d(with_box((900, 500, 1000, 560)), GAME)['over40'] >= 1, True)
check('a counter in the very corner does not', d(with_box((1880, 0, 1919, 40)), GAME)['worst'], 0)

m = Meter.__new__(Meter); m.args = type('A', (), {'replay': None})(); m.hwnd = 1
class Shared: screen_copy = True
keep = wc.shared, wc.window_copy
results.wc.shared = Shared
def proof(copy):
    results.wc.window_copy = lambda h: copy
    return m.game_alone(SHOWN)
SHOWN = with_box((700, 400, 1200, 700))
check('the game alone becomes the proof', proof(GAME) is GAME, True)
check('...and what it found goes with it', (m.screen_check['copy'], m.screen_check['over40'] > 10), ('ok', True))
for what, copy in (('black', Image.new('RGB', (W, H))), ('missing', None), ('another size', Image.new('RGB', (100, 100), (90, 90, 90)))):
    check(f'a copy {what}: the picture read stays', proof(copy) is SHOWN, True)
    check(f'...and says so ({what})', m.screen_check, {'source': 'screen', 'copy': 'failed'})
other = Image.new('RGB', (W, H), (250, 250, 250))
check('another screen: the picture read stays', proof(other) is SHOWN, True)
check('...with what it found', m.screen_check['copy'], 'ok')
Shared.screen_copy = False
check('window capture: no copy needed', proof(GAME) is SHOWN, True)
check('...it only ever sees the game', m.screen_check, {'source': 'window'})
results.wc.shared, results.wc.window_copy = keep

c = {'source': 'screen', 'copy': 'ok', 'worst': 200, 'over25': 12, 'over40': 11}
check('a run carries both', submit._screen_checks({'screen_check': c}, {'screen_check': {'source': 'window'}}), {'screenCheck': {'result': c, 'battleData': {'source': 'window'}}})
check('a run with none', submit._screen_checks({}, {}), {})
check('a match carries it', submit.egg_match_payload({'loot': 1, 'screen_check': c}, 'S1', '1')['extra']['screenCheck'], {'result': c})
check('a match with none', 'screenCheck' in submit.egg_match_payload({'loot': 1}, 'S1', '1')['extra'], False)

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('screen check: ok')
