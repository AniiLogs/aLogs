"""Holo-Battle Sim's exit gate button (sim_gate_pill): seen when the white letters split its pill in two.

    python tests/sim_gate_pill.py

13 Nov, at 4K on a brown ground: the letters' rows across the pill's middle left two bands too short to count, and the
button was never seen. A made-up pill picture, with and without the button. Exit code 1 on any failure.
"""
import os, sys, tempfile
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_pill_')
from PIL import Image, ImageDraw
import leaderboards as LB

def frame(pill=True):
    im = Image.new('RGB', (1920, 1080), (120, 96, 70))
    if pill:
        d = ImageDraw.Draw(im)
        d.rectangle((1222, 545, 1478, 588), fill=(67, 66, 80))
        for x in range(1270, 1460, 14): d.rectangle((x, 561, x + 8, 573), fill=(234, 233, 241))
    return im
box = (960, 432, 1728, 712.8)
fails = [what for what, got, want in (('pill with letters across it', LB.sim_gate_pill(frame(), box, 1920), True),
                                       ('no pill', LB.sim_gate_pill(frame(False), box, 1920), False)) if got != want]
if fails: print('FAIL ' + ', '.join(fails)); sys.exit(1)
print('sim_gate_pill: all checks pass')
