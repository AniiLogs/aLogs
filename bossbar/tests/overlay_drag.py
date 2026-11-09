"""A DPS window drawn with per-pixel alpha is dragged from where it really is, and a spot saved off screen is forgotten.

    python tests/overlay_drag.py

8 Nov: Windows moves that window (layered.show) and Tk still has it at -32000, so a drag from Tk's numbers threw it off
screen and saved that. Only the pure part is tested here: whether a saved offset lands in the game's area. The drag itself
needs windows on a desktop (checked by hand with them). Exit code 1 on any failure.
"""
import os, sys, tempfile
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_drag_')
from dps_overlay import offset_on_screen

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

W, H = 3840, 2160
check('near its spot', offset_on_screen((0.1, 0.1), 100, 100, 0, 0, W, H), True)
check('dragged across the screen', offset_on_screen((1.5, 0.8), 100, 100, 0, 0, W, H), True)
check('what the old drag saved', offset_on_screen((-14.8, -14.8), 100, 100, 0, 0, W, H), False)
check('far past the right', offset_on_screen((3.0, 0), 100, 100, 0, 0, W, H), False)
check('below the game', offset_on_screen((0, 2), 100, 100, 0, 0, W, H), False)
check('a game not at the corner', offset_on_screen((0.1, 0.1), 2100, 100, 1920, 0, 1920, 1080), True)
if fails:
    print('\n'.join(fails)); sys.exit(1)
print('overlay drag: ok')
