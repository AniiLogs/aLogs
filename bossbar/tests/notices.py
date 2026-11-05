"""The record notices over the game (live.py, Sarg's suggestion): the overlay's real notice code (tw ... toast_tick)
with its window calls stubbed, on a fake clock: timing, waiting while tabbed out, a backlog's one notice, each place
and motion, and the Custom spot (sample, drag, lock, 1440p, reset). Usage: python tests/notices.py"""
import json, os, re, sys, tempfile, types
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__))); sys.path.insert(0, HERE)
from PIL import Image, ImageDraw, ImageFont
src = open(os.path.join(HERE, 'live.py'), encoding='utf-8').read()
start = src.index("    tw = tk.Toplevel(root)")
end = src.index("        if a != toast['alpha'] and not AWAY['on']: layered.set_alpha(layered.top_hwnd(tw), a); toast['alpha'] = a\n")
block = '\n'.join(l[4:] for l in src[start:end].splitlines()) + \
    "\n    if a != toast['alpha'] and not AWAY['on']: layered.set_alpha(layered.top_hwnd(tw), a); toast['alpha'] = a\n"

clock = [1000.0]
calls = []
handlers = {}
class FakeWin:
    def overrideredirect(self, *a): pass
    def attributes(self, *a): pass
    def geometry(self, *a): pass
    def bind(self, ev, fn): handlers[ev] = fn
pos = {}; saved = []
def save_positions(): saved.append(dict(pos))
tk = types.SimpleNamespace(Toplevel=lambda root: FakeWin())
layered = types.SimpleNamespace(top_hwnd=lambda w: 1, click_through=lambda h, on=True: None,
                                set_alpha=lambda h, a: calls.append(('alpha', a)), park=lambda h: calls.append(('park',)))
def present(w, img, x, y): calls.append(('show', x, y))
def park(w): calls.append(('hide',))
meter = types.SimpleNamespace(hwnd=5, last_screen=(0, 0, 1920, 1080), args=types.SimpleNamespace(cfg={'leaderboard': {'submit': True}}))
AWAY = {'on': False}; hide = False; OFF = '+-32000+-32000'; root = None
rgb = lambda c: tuple(int(c[i:i + 2], 16) for i in (1, 3, 5))
log = types.SimpleNamespace(info=lambda m: calls.append(('log', m)))
time = types.SimpleNamespace(time=lambda: clock[0])
NOTICES = tempfile.mkdtemp(prefix='notices-')
exec(block)

def run(seconds, step=0.015):
    t_end = clock[0] + seconds
    while clock[0] < t_end: toast_tick(); clock[0] += step

def check(name, ok):
    print(('PASS ' if ok else 'FAIL ') + name)
    if not ok: global failed; failed = True
failed = False

# 1. one notice: fades in, stays, fades out, hides after 4 s, at the top right under Sanctum Helper's notices
toast['queue'].append(('Interlink run sent', 'Alpha Scorchhowl · 17 stars · 17,061', '#7fb2ff'))
calls.clear(); run(4.2)
shows = [c for c in calls if c[0] == 'show']; alphas = [c[1] for c in calls if c[0] == 'alpha']
check('shown once, top right', len(shows) == 1 and shows[0][1] == 1920 - 360 - 18 and shows[0][2] == 92)
check('fades in from 0 to 255 then out to near 0', alphas[0] < 60 and max(alphas) == 255 and alphas[-1] < 60)
check('hidden after 4 s', ('hide',) in calls and toast['since'] is None)

# 2. tabbed out: waits, then shows when the game is back
AWAY['on'] = True; toast['queue'].append(('Egg Heist rank updated', 'Gold II · 1,337', '#ffc861'))
calls.clear(); run(2)
check('waits while tabbed out', not [c for c in calls if c[0] == 'show'])
AWAY['on'] = False; run(0.1)
check('shows once back in the game', [c for c in calls if c[0] == 'show'])
run(5)

# 3. a backlog of 5: one notice with the count
for i in range(5): toast['queue'].append(('Interlink run sent', f'run {i}', '#7fb2ff'))
calls.clear(); run(0.1)
check('backlog of 5 makes one notice with the count', any(c[0] == 'log' and '5 records sent' in c[1] for c in calls) and not toast['queue'])
run(5)

# 4. two waiting: one after the other
toast['queue'] += [('A sent', 'x', '#7fb2ff'), ('B sent', 'y', '#7fb2ff')]
calls.clear(); run(9)
logs = [c[1] for c in calls if c[0] == 'log']
check('two waiting show one after the other', len(logs) == 2 and logs[0].startswith('notice: A') and logs[1].startswith('notice: B'))

# 5. slide from the right: starts off to the right, ends at its spot
meter.args.cfg['leaderboard'] = {'submit': True, 'notify_motion': 'slide', 'notify_place': 'top-right'}
toast['queue'].append(('Slide sent', 'x', '#7fb2ff')); calls.clear(); run(1.0)
xs = [c[1] for c in calls if c[0] == 'show']
check('slide comes in from the right edge', xs[0] > 1700 and xs[-1] == 1920 - 360 - 18 and xs == sorted(xs, reverse=True))
run(4)

# 6. drop at top middle: starts above, ends at its spot
meter.args.cfg['leaderboard'] = {'submit': True, 'notify_motion': 'drop', 'notify_place': 'top-center'}
toast['queue'].append(('Drop sent', 'x', '#7fb2ff')); calls.clear(); run(1.0)
ys = [c[2] for c in calls if c[0] == 'show']
check('drop comes down from the top to 150', ys[0] < 0 and ys[-1] == 150 and ys == sorted(ys))
run(4)

# 7. slide at middle left: from the left edge
meter.args.cfg['leaderboard'] = {'submit': True, 'notify_motion': 'slide', 'notify_place': 'left'}
toast['queue'].append(('Left sent', 'x', '#7fb2ff')); calls.clear(); run(1.0)
xs = [c[1] for c in calls if c[0] == 'show']
check('middle left slides in from the left', xs[0] < 0 and xs[-1] == 18)
run(4)

# 8. fade keeps still: one position only
meter.args.cfg['leaderboard'] = {'submit': True, 'notify_motion': 'fade', 'notify_place': 'right'}
toast['queue'].append(('Fade sent', 'x', '#7fb2ff')); calls.clear(); run(4.2)
check('fade stays in one spot (middle right)', len({(c[1], c[2]) for c in calls if c[0] == 'show'}) == 1)

run(1)
# 9. custom, unlocked: the sample shows and stays; a notice waits
meter.args.cfg['leaderboard'] = {'submit': True, 'notify_place': 'custom', 'notify_locked': False, 'notify_motion': 'fade'}
toast['queue'].append(('Waiting sent', 'x', '#7fb2ff')); calls.clear(); run(6)
shows = [c for c in calls if c[0] == 'show']
check('unlocked custom shows the sample at the top right, once', len(shows) == 1 and shows[0][1] == 1920 - 360 - 18 and toast['sample'])
check('a notice waits while placing', toast['queue'] and not any(c[0] == 'log' for c in calls))
# drag it to (400, 700) and drop
E = lambda x, y: types.SimpleNamespace(x_root=x, y_root=y)
sx, sy = toast['sample'][0], toast['sample'][1]
handlers['<Button-1>'](E(sx + 10, sy + 10)); handlers['<B1-Motion>'](E(410, 710)); handlers['<ButtonRelease-1>'](E(410, 710))
check('dropping saves the spot as a share of the height', saved and abs(pos['notice'][0] - 400 / 1080) < 1e-9 and abs(pos['notice'][1] - 700 / 1080) < 1e-9)
calls.clear(); run(0.1)
shows = [c for c in calls if c[0] == 'show']
check('the sample shows again where it was dropped', shows and shows[-1][1:] == (400, 700))
# lock it: the sample goes, the waiting notice shows at the custom spot and slides from the left (nearer edge)
meter.args.cfg['leaderboard'] = {'submit': True, 'notify_place': 'custom', 'notify_locked': True, 'notify_motion': 'slide'}
calls.clear(); run(1.0)
shows = [c for c in calls if c[0] == 'show']
check('locked: the notice shows at the custom spot, sliding in from the left', ('hide',) in calls and shows[0][1] < 400 and shows[-1][1:] == (400, 700))
run(4)
# a different screen size: the spot keeps its share (1440p: 400/1080*1440)
meter.last_screen = (0, 0, 2560, 1440); toast['queue'].append(('Big sent', 'x', '#7fb2ff'))
meter.args.cfg['leaderboard']['notify_motion'] = 'fade'; calls.clear(); run(1)
shows = [c for c in calls if c[0] == 'show']
check('the custom spot scales with the game (1440p)', shows and shows[-1][1:] == (round(400 / 1080 * 1440), round(700 / 1080 * 1440)))
run(4); meter.last_screen = (0, 0, 1920, 1080)
# double-click the sample: back to the top right
meter.args.cfg['leaderboard'] = {'submit': True, 'notify_place': 'custom', 'notify_locked': False}
run(0.1); handlers['<Double-Button-1>'](E(0, 0)); calls.clear(); run(0.1)
shows = [c for c in calls if c[0] == 'show']
check('double-click puts it back top right', 'notice' not in pos and shows and shows[-1][1] == 1920 - 360 - 18)


# 10. Sanctum Helper's notice file: shown in the same system, then deleted
meter.args.cfg['leaderboard'] = {'submit': True, 'notify_place': 'top-right', 'notify_motion': 'fade'}
run(5)
with open(os.path.join(NOTICES, '0001.json'), 'w', encoding='utf-8') as f:
    json.dump({'head': 'Chaos Mode found.', 'detail': 'Sanctum Helper', 'accent': '#e05a5a', 'screen': [0, 0, 1920, 1080], 'from': 'sanctum'}, f)
calls.clear(); run(0.5)
check('a Sanctum Helper notice file is shown and deleted', any(c[0] == 'log' and 'Chaos Mode found.' in c[1] for c in calls) and not os.listdir(NOTICES))
run(5)
# 11. in a homeland the meter may not know the game window: the notice's own screen places it (a 2560x1440 game at 100,50)
meter.last_screen = None
with open(os.path.join(NOTICES, '0002.json'), 'w', encoding='utf-8') as f:
    json.dump({'head': 'Hard Mode found.', 'detail': 'Sanctum Helper', 'accent': '#5cb8ff', 'screen': [100, 50, 2560, 1440]}, f)
calls.clear(); run(0.5)
shows = [c for c in calls if c[0] == 'show']
check("without the meter's own screen, the notice's screen places it", shows and shows[-1][1:] == (100 + 2560 - round(360 * 4 / 3) - round(18 * 4 / 3), 50 + round(92 * 4 / 3)))
run(5); meter.last_screen = (0, 0, 1920, 1080)
# 12. three records waiting with a Sanctum notice after them: one count, then the Sanctum notice
toast['queue'] += [('R1 sent', 'x', '#7fb2ff'), ('R2 sent', 'x', '#7fb2ff'), ('R3 sent', 'x', '#7fb2ff')]
with open(os.path.join(NOTICES, '0003.json'), 'w', encoding='utf-8') as f: json.dump({'head': 'Nightmare Mode found.', 'from': 'sanctum'}, f)
calls.clear(); run(9)
logs = [c[1].split(' - ')[0] for c in calls if c[0] == 'log']
check('a backlog of records is one count, the Sanctum notice after it', logs == ['notice: 3 records sent to the leaderboards', 'notice: Nightmare Mode found.'])
run(1)
# a Sanctum notice between records: each on its own, in order
toast['queue'] += [('R1 sent', 'x', '#7fb2ff'), ('R2 sent', 'x', '#7fb2ff')]
calls.clear(); run(0.3)                                                    # R1 shows
with open(os.path.join(NOTICES, '0005.json'), 'w', encoding='utf-8') as f: json.dump({'head': 'Hard Mode found.', 'from': 'sanctum'}, f)
run(0.3); toast['queue'].append(('R3 sent', 'x', '#7fb2ff')); run(17)
logs = [c[1].split(' - ')[0] for c in calls if c[0] == 'log']
check('a Sanctum notice between records keeps its place', logs == ['notice: R1 sent', 'notice: R2 sent', 'notice: Hard Mode found.', 'notice: R3 sent'])
# 13. a bad file is dropped, nothing breaks
with open(os.path.join(NOTICES, '0004.json'), 'w', encoding='utf-8') as f: f.write('{not json')
calls.clear(); run(0.5)
check('an unreadable notice file is deleted and ignored', not os.listdir(NOTICES) and not any(c[0] == 'log' for c in calls))

# 14. each plug-in owns its notices: one switched off drops its own, the others still show
meter.args.cfg['leaderboard']['submit'] = False
meter.args.cfg['sanctum'] = {'enabled': True}
toast['queue'].append(('R9 sent', 'x', '#7fb2ff'))
with open(os.path.join(NOTICES, '0006.json'), 'w', encoding='utf-8') as f: json.dump({'head': 'Hard Mode found.', 'from': 'sanctum'}, f)
with open(os.path.join(NOTICES, '0007.json'), 'w', encoding='utf-8') as f: json.dump({'head': 'aLogs off', 'from': 'button'}, f)
calls.clear(); run(9)
logs = [c[1].split(' - ')[0] for c in calls if c[0] == 'log']
check('leaderboards off: their record dropped; Sanctum Helper and the button still show', logs == ['notice: Hard Mode found.', 'notice: aLogs off'])
meter.args.cfg['sanctum'] = {'enabled': False}; meter.args.cfg['notifications'] = {'button': False}
with open(os.path.join(NOTICES, '0008.json'), 'w', encoding='utf-8') as f: json.dump({'head': 'Hard Mode found.', 'from': 'sanctum'}, f)
with open(os.path.join(NOTICES, '0009.json'), 'w', encoding='utf-8') as f: json.dump({'head': 'aLogs on', 'from': 'button'}, f)
calls.clear(); run(2)
check("Sanctum Helper off and the button's notice off: neither shows", not any(c[0] == 'log' for c in calls))

print('ALL PASS' if not failed else 'SOME FAILED'); sys.exit(1 if failed else 0)
