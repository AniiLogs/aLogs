"""Holo-Battle Sim's exit gate: the button going away (after it was up) is your press, and aLogs takes control.

    python tests/sim_gate_gone.py

The owner, 13 Nov: as soon as the "End Holo-Battle Sim" button goes away, take control (then the Victory screen and
the Gizmo screen follow). Gone for SIM_GATE_PRESSED counts; a single look that misses it doesn't. The button check
itself on a frame where the white letters split the pill (sim_gate_pill) is covered by a made-up pill picture.
Exit code 1 on any failure.
"""
import os, sys, tempfile, types
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_gate_')
from PIL import Image, ImageDraw
import live, leaderboards as LB

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

# a 1920x1080 frame: brown ground, the button's flat blue-grey pill with a band of white letters across its middle
def frame(pill=True):
    im = Image.new('RGB', (1920, 1080), (120, 96, 70))
    if pill:
        d = ImageDraw.Draw(im)
        d.rectangle((1222, 545, 1478, 588), fill=(67, 66, 80))
        for x in range(1270, 1460, 14): d.rectangle((x, 561, x + 8, 573), fill=(234, 233, 241))
    return im
check('pill with letters across it is seen', LB.sim_gate_pill(frame(), (960, 432, 1728, 712.8), 1920), True)
check('no pill: not seen', LB.sim_gate_pill(frame(False), (960, 432, 1728, 712.8), 1920), False)

# the gate's flow, with the screen reads stubbed: the button up, then gone
m = live.Meter.__new__(live.Meter)
m.holosim_space = True; m.space = 'sim-space'; m.hwnd = 1
m.args = types.SimpleNamespace(replay=None, cfg={'leaderboard': {'submit': True, 'automation': True},
                                                 'results': {'sim_gate': True, 'hold_inputs': False, 'hold_seconds': 60}})
m.sim_tour = None; m.sim_ctl = None
LB.wc.is_foreground = lambda hwnd: True
shown = {'up': True}
LB.sim_gate_pill = lambda im, b, u: shown['up']
m.ocr = types.SimpleNamespace(read=lambda crops: {'l': 'Level 16/16'} if 'l' in crops else {'b': 'End Holo-Battle Sim' if shown['up'] else ''})
class Watch:
    last = None
    def start(self): pass
    def stop(self): pass
m.sim_press = Watch()
taken = []
m.sim_control = lambda now, why: (taken.append(why), setattr(m, 'sim_ctl', {'why': why}), m.sim_ctl)[2]
im = frame()
t = 100.0
for _ in range(4): m.sim_gate(im, t); t += 0.2                       # the last level read, the button up
check('nothing taken while the button is up', taken, [])
shown['up'] = False
m.sim_gate(im, t); t += 0.2                                          # one look without it
check('one look without the button: not a press', taken, [])
shown['up'] = True; m.sim_gate(im, t); t += 0.2                      # back: it was only a missed look
shown['up'] = False
for _ in range(4): m.sim_gate(im, t); t += 0.2                       # gone for good
check('button gone: control taken once', taken, ['the exit gate button went away (your press)'])
check('waiting for the Victory screen', (m.sim_ctl or {}).get('stage'), 'gate')

if fails:
    print('FAIL\n  ' + '\n  '.join(fails)); sys.exit(1)
print('sim_gate_gone: all checks pass')
