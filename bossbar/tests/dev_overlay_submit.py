"""Beta Menu > Dev: the overlay's and the sender's switches, each on and off.

    python tests/dev_overlay_submit.py

- overlay-label-cache: off draws and shows as ever; on, a kept picture is the one a fresh draw gives, a window is only
  left alone while it shows that very picture at that spot, and the whole window loop, run over a made-up fight, puts
  the same pixels at the same spots at every look;
- overlay-idle-poll: off looks every 15 ms; on, 50 ms only while nothing is up and no fight is on, the same pictures in
  the same order, what begins with nothing up no more than 50 ms later, a fight's numbers redrawn 4 times a second as ever;
- queue-wait-fight: off retries on the minute whatever is going on; on, a retry waits for the fight, a new record
  doesn't, nothing waits longer than its ceiling, and every record is still sent;
- proof-encode-once: off encodes on every try; on, once, and what's sent is byte for byte what a fresh encode gives.
No Tk windows (a stand-in for Tk and for the layered windows), made-up fights and pictures, sending stubbed.
Exit code 1 on any failure.
"""
import copy, hashlib, io, itertools, json, logging, os, random, re, sys, tempfile, types
import time as real_time
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
TMP = tempfile.mkdtemp(prefix='alogs_devos_')
os.environ['ALOGS_DATA'] = TMP
os.environ.pop('ALOGS_DEV_ON', None)
import PIL
from PIL import Image, ImageDraw
import devflags as D
import settings as C
import dps_overlay as O
import notice_window as NW
import layered, gamefont, submit, leaderboards
import wincapture as wc

LINES, ERRORS = [], []          # what meter.log would get: every line, and the ones that are warnings or worse
class Keep(logging.Handler):
    def emit(self, record):
        LINES.append(record.getMessage())
        if record.levelno >= logging.WARNING: ERRORS.append(record.getMessage() + ' ' + repr(record.exc_info and record.exc_info[1]))
logging.getLogger('meter').addHandler(Keep()); logging.getLogger('meter').propagate = False
logging.getLogger('meter').setLevel(logging.INFO)

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')
def said(start): return [l for l in LINES if l.startswith(start)]

FLAGS = os.path.join(TMP, 'dev-on.json')
def switches(*on):
    """As the app writes it: whole, then the meter looks."""
    with open(FLAGS + '.tmp', 'w', encoding='utf-8') as f: json.dump({'on': list(on)}, f)
    os.replace(FLAGS + '.tmp', FLAGS)
    D.use(FLAGS)

# ---------------------------------------------------------------- overlay-label-cache: the kept pictures
r = O.Reuse(); made = []
def maker(k): return lambda: (made.append(k), ('picture', k))[1]
a = r.label('k', maker('k')); b = r.label('k', maker('k'))
check('label: asked again, the same picture', a is b, True)
check('label: drawn once', made, ['k'])
r = O.Reuse(); made = []
for i in range(O.Reuse.KEEP): r.label(i, maker(i))
r.label(0, maker(0)); r.label('new', maker('new'))
check('label: never more than KEEP kept', len(r.labels), O.Reuse.KEEP)
check('label: the one just used stays, the oldest goes', (0 in r.labels, 1 in r.labels, 'new' in r.labels), (True, False, True))
for i in range(500): r.label(('more', i), maker(i))
check('label: still no more than KEEP after 500 others', len(r.labels), O.Reuse.KEEP)
check('label: counted', (r.n['drawn'], r.n['reused']), (O.Reuse.KEEP + 501, 1))

text = Image.new('RGBA', (90, 22), (0, 0, 0, 0)); ImageDraw.Draw(text).text((3, 3), 'HP 1.20M / 2.00M', fill=(255, 255, 255, 255))
one, two = O.with_shadow(text, 22), O.with_shadow(text.copy(), 22)
check('a label drawn twice is the same picture, so a kept one is what a fresh draw gives',
      (one[0].tobytes(), one[0].size, one[1]), (two[0].tobytes(), two[0].size, two[1]))
check('the shadow leaves the text it was given as it was', text.tobytes(), text.copy().tobytes())

# ---------------------------------------------------------------- overlay-label-cache: windows left as they are
# Every way a window is asked to show something, at random: with Reuse in between, each window ends up showing exactly
# what it does when every request is carried out.
rng = random.Random(7); pics = [Image.new('RGBA', (4, 4), (i, 0, 0, 255)) for i in range(3)]
r = O.Reuse(); plain, reused, asked = {}, {}, {}; now = 100.0; calls = 0; same = True
def carry(hwnd, image, x, y):
    global calls
    calls += 1; reused[hwnd] = (id(image), int(x), int(y)); return True
for step in range(4000):
    now += rng.choice((0.015, 0.015, 0.25, 1.0)); win = rng.choice('abc'); act = rng.random()
    if act < 0.80:                                                               # mostly what it showed last, as in a fight
        image, x, y = asked[win] if act < 0.55 and win in asked else (rng.choice(pics), rng.choice((10, 10.4, 11)), rng.choice((20, 21)))
        asked[win] = (image, x, y)
        plain[win] = (id(image), int(x), int(y)); r.show(carry, win, win, image, x, y, now)
    elif act < 0.93: plain[win] = reused[win] = 'parked'; r.forget(win)         # parked, or moved by Tk
    else:                                                                        # tabbed out: every window
        for w in 'abc': plain[w] = reused[w] = 'parked'
        r.forget()
    same = same and plain == reused
check('windows: the same on screen after every one of 4000 random requests', same, True)
check('windows: and most requests left the window alone', (r.n['left'] > 500, calls == r.n['shown'], calls + r.n['left'] > 3000),
      (True, True, True))

r = O.Reuse(); shown = []; ok = [True]
def carry(hwnd, image, x, y): shown.append((id(image), x, y)); return ok[0]
p = pics[0]; twin = p.copy()
r.show(carry, 'w', 1, p, 5, 6, 10.0); r.show(carry, 'w', 1, p, 5, 6, 10.2)
check('windows: the same picture at the same spot is shown once', len(shown), 1)
r.show(carry, 'w', 1, twin, 5, 6, 10.3)
check('windows: another picture with the same pixels is shown (no guessing)', len(shown), 2)
r.show(carry, 'w', 1, twin, 6, 6, 10.4)
check('windows: the same picture somewhere else is shown', len(shown), 3)
r.show(carry, 'w', 1, twin, 6, 6, 10.4 + O.Reuse.REFRESH_S - 0.01)
check('windows: left alone until REFRESH_S', len(shown), 3)
r.show(carry, 'w', 1, twin, 6, 6, 10.4 + O.Reuse.REFRESH_S)
check('windows: shown again every REFRESH_S, whatever happened to the window meanwhile', len(shown), 4)
r.show(carry, 'w', 1, twin, 6, 6, 5.0)
check('windows: a clock that went backwards shows it', len(shown), 5)
r.forget('w'); r.show(carry, 'w', 1, twin, 6, 6, 5.1)
check('windows: one that was parked is shown', len(shown), 6)
ok[0] = False; r.forget(); check('windows: a show that failed says so', r.show(carry, 'w', 1, twin, 6, 6, 5.2), False)
ok[0] = True; r.show(carry, 'w', 1, twin, 6, 6, 5.3)
check('windows: and is tried again', len(shown), 8)
check('windows: a second window is its own', (r.show(carry, 'v', 2, twin, 6, 6, 5.3), len(shown)), (True, 9))

LINES.clear(); r = O.Reuse(); r.tell(0.0)
r.label('k', lambda: 1); r.label('k', lambda: 1); r.show(lambda *a: True, 'w', 1, p, 0, 0, 1.0); r.show(lambda *a: True, 'w', 1, p, 0, 0, 1.1)
r.tell(59.0); check('label cache: no line before the minute', said('overlay-label-cache'), [])
r.tell(60.0)
check('label cache: its line, counts and milliseconds only',
      [re.sub(r'\(\d+ ms\)', '(N ms)', l) for l in said('overlay-label-cache')],
      ['overlay-label-cache: labels 1 drawn (N ms), 1 reused; windows 1 shown (N ms), 1 left as they were (last 60 s)'])
r.tell(125.0); check('label cache: nothing counted, no line', len(said('overlay-label-cache')), 1)

# ---------------------------------------------------------------- overlay-idle-poll: when nothing is up
def quiet():
    return dict(meter=types.SimpleNamespace(fight=None, hit_at=0.0), placed={}, waiting=False, dragging=False,
                drawn={'at': 0.0, 'pending': None}, dps_at={'hide_at': None},
                alert={'since': None}, toast={'since': None, 'queue': [], 'sample': None, 'drag': None}, now=1000.0)
check('idle: nothing up, nothing waiting', O.overlay_idle(**quiet()), True)
for what, change in (('a fight', lambda k: setattr(k['meter'], 'fight', object())),
                     ('a window is up', lambda k: k['placed'].update(w=('img', None, 0, 0))),
                     ('a message from the meter waits', lambda k: k.update(waiting=True)),
                     ('a window is held', lambda k: k.update(dragging=True)),
                     ('a reading waits to be drawn', lambda k: k['drawn'].update(pending={})),
                     ('a result or note is timed to go', lambda k: k['dps_at'].update(hide_at=1001.0)),
                     ('the example is on', lambda k: k['dps_at'].update(example=True)),
                     ('the alert is up', lambda k: k['alert'].update(since=999.0)),
                     ('a notice is up', lambda k: k['toast'].update(since=999.0)),
                     ('a notice waits', lambda k: k['toast']['queue'].append({})),
                     ('the sample notice is up', lambda k: k['toast'].update(sample=[0, 0, None])),
                     ('the sample notice is held', lambda k: k['toast'].update(drag=(1, 1))),
                     ('a hit in the last 5 s', lambda k: setattr(k['meter'], 'hit_at', 996.0)),
                     ('a proof is being taken', lambda k: setattr(k['meter'], 'proof_until', 1000.5))):
    k = quiet(); change(k); check(f'not idle: {what}', O.overlay_idle(**k), False)

LINES.clear(); p = O.Pace()
check('pace: 50 ms while idle, 15 ms otherwise', [p.wait(True, 100.0), p.wait(False, 100.05), p.wait(True, 130.0)], [50, 15, 50])
check('pace: no line before the minute', said('overlay-idle-poll'), [])
p.wait(True, 160.0); p.wait(False, 161.0)
check('pace: its line, counts only', said('overlay-idle-poll'), ['overlay-idle-poll: 3 looks at 50 ms (nothing up), 1 at 15 ms (last 60 s)'])
check('the two paces', (O.POLL_MS, O.IDLE_MS), (15, 50))

# ---------------------------------------------------------------- the window loop itself, over a made-up fight
# run_overlay as it is, with stand-ins for Tk, the layered windows, the game font, the clock and the meter's thread.
# Time only moves when the stand-in for Tk's loop moves it, so every run lives the same 80 seconds: a notice, a fight
# (hits landing, then none, Interlink's bars, tabbed out and hit, the boss bar out of sight, the window dragged), its
# result, a note on an end screen, the example, and a hit while nothing is up.
T0, END_MS, OFFSCREEN = 1000.0, 80_000, -32000
NAMES = ['dps', 'pct', 'bar', 'total', 'break', 'alert', 'edge', 'notice']       # the windows, in the order they're made
MISSING = object()

class Clock:
    def __init__(self): self.ms = 0
    def time(self): return T0 + self.ms / 1000
    def monotonic(self): return 50.0 + self.ms / 1000
    def sleep(self, s): pass
    def __getattr__(self, name): return getattr(real_time, name)

class Screen:
    """What would be on screen: each window's picture (its pixels' md5 and its size), spot and opacity."""
    def __init__(self, clock):
        self.clock = clock; self.wins = {}; self.shows = []; self.changes = {n: [] for n in NAMES}
    def add(self, win):
        win.name = NAMES[len(self.wins)]
        self.wins[win.id] = {'name': win.name, 'pic': None, 'at': (OFFSCREEN, OFFSCREEN), 'alpha': 255}
    def _now(self, w, pic, at):
        if (pic, at) != (w['pic'], w['at']): self.changes[w['name']].append((self.clock.ms, pic, at))
        w['pic'], w['at'] = pic, at
    def top_hwnd(self, win): return win.id
    def show(self, hwnd, image, x, y):                       # layered.show: the picture, the spot, fully opaque
        w = self.wins[hwnd]; rgba = image.convert('RGBA')
        self._now(w, (hashlib.md5(rgba.tobytes()).hexdigest(), rgba.size), (int(x), int(y))); w['alpha'] = 255
        self.shows.append((self.clock.ms, w['name'])); return True
    def set_alpha(self, hwnd, alpha): self.wins[hwnd]['alpha'] = max(0, min(255, int(alpha)))
    def park(self, hwnd): w = self.wins[hwnd]; self._now(w, w['pic'], (OFFSCREEN, OFFSCREEN))
    def click_through(self, hwnd, on=True): pass
    def geometry(self, hwnd, x, y): w = self.wins[hwnd]; self._now(w, w['pic'], (x, y))    # moved by Tk
    def state(self): return tuple((w['name'], w['pic'], w['at'], w['alpha']) for w in self.wins.values())
    def up(self): return tuple(w['name'] for w in self.wins.values() if w['pic'] and w['at'][0] > OFFSCREEN and w['alpha'] > 0)
    def at(self, name): return next(w['at'] for w in self.wins.values() if w['name'] == name)

class FakeFont:
    """In place of the game's font: a picture decided by everything render() is given, the same every time, and kept as
    the real one keeps its own (asked the same again, the very same picture comes back)."""
    def __init__(self): self.cache = {}
    def render(self, text, px, fill=(255, 255, 255), outline=(20, 26, 44), italic=False, bold=False, stroke=False, lead=None):
        key = (text, px, fill, outline, italic, bold, stroke, lead)
        if key in self.cache: return self.cache[key]
        cw = max(3, px * 6 // 10); h = px + 6
        im = Image.new('RGBA', (cw * max(1, len(text)) + 4, h), (0, 0, 0, 0)); d = ImageDraw.Draw(im)
        for i, ch in enumerate(text):
            if ch == ' ': continue
            colour = tuple(lead[1]) if lead and i < lead[0] else tuple(fill)
            d.rectangle((2 + i * cw, 2 + ord(ch) % 4, i * cw + cw, h - 4), fill=colour + (255,),
                        outline=tuple(outline) + (255 if stroke else 200,))
        if bold: d.line((0, 0, im.width - 1, 0), fill=tuple(fill) + (255,))
        for i, v in enumerate(hashlib.md5(repr(key).encode('utf-8')).digest()[:min(16, im.width)]):
            im.putpixel((i, h - 1), (v, 255 - v, v, 255))                       # (no two askings look alike)
        im.info['base'] = h - 4; im.info['italic'] = italic
        self.cache[key] = im; return im

class NoThread:
    """The meter's thread and the backstop don't run; the font loads at once."""
    def __init__(self, target=None, daemon=None, **kw): self.target = target
    def start(self):
        if getattr(self.target, '__name__', '') == 'load_font': self.target()
    def is_alive(self): return True
    def join(self, timeout=None): pass

def fake_tk(world):
    """Tk, as far as the overlay asks: windows that only remember what was bound to them and where Tk put them, and a
    loop that moves the clock to each look and lets the made-up world happen first."""
    ids = itertools.count(1000)
    class Widget:
        def __init__(self, master=None, **kw): self.binds = {}
        def bind(self, event, fn): self.binds[event] = fn
        def pack(self, **kw): pass
        def delete(self, *a): pass
        def config(self, **kw): pass
        configure = config
        def update_idletasks(self): pass
        def winfo_reqwidth(self): return 1
        def winfo_reqheight(self): return 1
    class Toplevel(Widget):
        def __init__(self, master=None, **kw):
            Widget.__init__(self); self.id = next(ids); world.screen.add(self); world.windows[self.name] = self
        def overrideredirect(self, on): pass
        def attributes(self, *a): pass
        def geometry(self, where):
            x, y = re.fullmatch(r'\+(-?\d+)\+(-?\d+)', where).groups(); world.screen.geometry(self.id, int(x), int(y))
        def winfo_id(self): return self.id
        def winfo_x(self): return OFFSCREEN
        def winfo_y(self): return OFFSCREEN
    class Canvas(Widget): pass
    class Tk(Widget):
        def withdraw(self): pass
        def winfo_children(self): return []
        def winfo_screenwidth(self): return 1280
        def winfo_screenheight(self): return 720
        def destroy(self): world.pending = None
        def after(self, ms, fn):
            # each look: when, how long until the next, everything on screen, whether a fight is on, which windows are up
            world.ticks.append((world.clock.ms, ms, world.screen.state(), bool(world.meter.fight), world.screen.up()))
            world.pending = (world.clock.ms + ms, fn)
        def mainloop(self):
            while world.pending and world.pending[0] <= END_MS:
                at, fn = world.pending; world.pending = None; world.clock.ms = at
                while world.events and world.events[0][0] <= at: world.events.pop(0)[1]()
                fn()
    tk = types.ModuleType('tkinter'); tk.Tk, tk.Toplevel, tk.Canvas = Tk, Toplevel, Canvas
    font = types.ModuleType('tkinter.font'); font.families = lambda root=None: (); tk.font = font
    return tk, font

BAR = (400, 60, 500)                                   # the boss bar: left bracket x, centre y, bracket span
def reading(ms):
    """What the meter says at this moment of the made-up fight (6 s to 30 s): a new dict every time, its numbers
    decided by the half second it falls in."""
    k = ms // 500 - 12; s = k / 2                      # half seconds, and seconds, since the fight began
    d = {'screen': (0, 0, 1280, 720), 'bar': BAR, 'max_hp': 2_000_000, 'boss_hp': None, 'bossrush': False, 'xn': 0,
         'seg_size': 0, 'seg_left': 0, 'title': None}
    if k < 12:      # hits land: every number moves
        d.update(damage=20_000 * k, elapsed=s, brk={'left': 0.9 - 0.02 * k, 'rate': 1.5, 'size': 4000})
    elif k < 20:    # none land (the boss is away): the time runs on, the labels on the bar don't change
        d.update(damage=240_000, elapsed=s, brk={'left': 0.0, 'breaks': 1, 'phase': 'broken'})
    elif k < 28:    # nothing changes at all
        d.update(damage=240_000, elapsed=10.0, brk={'left': 0.0, 'breaks': 1, 'phase': 'refill'})
    elif k < 36:    # Interlink: this bar's HP too, the second window spot, the orange gauge
        n = k - 28
        d.update(bossrush=True, xn=3, seg_size=500_000, seg_left=500_000 - 15_000 * n, damage=240_000 + 15_000 * n,
                 elapsed=s, boss=(1, 2), brk={'left': 0.8 - 0.05 * n, 'kind': 'orange'})
    elif k < 40:    # the same text under the bar, in another colour each time (the bar's colour where its HP ends)
        d.update(bossrush=True, xn=3, seg_size=500_000, seg_left=380_000 - 100_000 * (k - 36), damage=360_000,
                 elapsed=15.0, boss=(1, 2), brk={})
    elif k < 44:    # out in the world again
        n = k - 40
        d.update(damage=360_000 + 30_000 * n, elapsed=s, brk={'left': 0.6 - 0.1 * n, 'rate': 1.0, 'rate_now': 2.5, 'breaks': 2, 'size': 4000})
    else:           # nothing changes: the window is dragged meanwhile
        d.update(damage=480_000, elapsed=22.0, brk={'left': 0.2, 'rate': 1.0, 'rate_now': 2.5, 'breaks': 2, 'size': 4000})
    d['break'] = d.pop('brk'); d['avg'] = d['damage'] / max(1.0, d['elapsed']); d['recent'] = d['avg'] * 1.25
    return d

STARTS = {'notice': 1010, 'fight': 6005, 'note': 41_000, 'example': 62_010, 'hit': 70_005}   # things beginning with nothing up
def scenario(world):
    """[(ms, what happens then)]: the same for every run. Each carries its own time (the boss bar is "seen" when its
    reading was made, not when the overlay got round to it), away from the half seconds the numbers change on."""
    meter = world.meter; overlay = meter.args.cfg['overlay']; ev = []
    def at(ms, fn): ev.append((ms, fn))
    def notice(): meter.out('toast', {'head': 'Made-up notice', 'detail': 'for the test', 'accent': '#5cb8ff', 'screen': None, 'kind': 'test'})
    def start(): meter.fight = object(); meter.last_screen = (0, 0, 1280, 720)
    def read(ms, seen):
        def go():
            if seen: meter.bar_seen = T0 + ms / 1000
            meter.out('fight', reading(ms))
        return go
    def front(hwnd, hit=None):
        def go():
            world.front[0] = hwnd
            if hit is not None: meter.hit_at = T0 + hit / 1000
        return go
    def mouse(event, dx=0, dy=0):
        def go():
            if event == '<Button-1>': world.grab = world.screen.at('dps')
            x, y = world.grab
            world.windows['dps'].binds[event](types.SimpleNamespace(x_root=x + 5 + dx, y_root=y + 5 + dy))
        return go
    def end():
        d = reading(29_999); d.update(bossrush=False, defeated=True, title='Made-up Boss')
        meter.fight = None; meter.out('fight_end', d)
    def example(on):
        def go(): overlay['dps_example'] = on; meter.frame_size = (1280, 720)
        return go
    at(STARTS['notice'], notice)
    at(STARTS['fight'], start)
    for ms in range(6005, 30_000, 33): at(ms, read(ms, not 26_400 <= ms < 27_400))    # (the bar out of sight for a second)
    at(22_400, front(999, 22_400)); at(24_900, front(500))                            # tabbed out and hit, then back
    at(28_050, lambda: overlay.update(dps_locked=False))
    at(28_555, mouse('<Button-1>'))
    for i in range(1, 11): at(28_555 + 15 * i, mouse('<B1-Motion>', 3 * i, 2 * i))
    at(28_750, mouse('<ButtonRelease-1>', 30, 20))
    at(29_450, lambda: overlay.update(dps_locked=True))
    at(30_000, end)
    at(33_020, notice)                                                               # while the result is up
    at(STARTS['note'], lambda: meter.out('status', 'Battle Data saved: game team DPS 21,818 vs meter 21,800'))
    at(44_000, lambda: meter.out('end_hide', None))
    at(STARTS['example'], example(True)); at(66_000, example(False))
    at(STARTS['hit'], front(999, STARTS['hit'])); at(76_000, front(500))              # hit while tabbed out, nothing up
    ev.sort(key=lambda e: e[0])
    return ev

class Patch:
    """Stand-ins put in place, and every one of them taken out again."""
    def __init__(self): self.undo = []
    def attr(self, holder, name, value):
        self.undo.append((holder, name, vars(holder).get(name, MISSING))); setattr(holder, name, value)
    def module(self, name, value):
        self.undo.append((None, name, sys.modules.get(name, MISSING))); sys.modules[name] = value
    def back(self):
        for holder, name, old in reversed(self.undo):
            if holder is None:
                if old is MISSING: sys.modules.pop(name, None)
                else: sys.modules[name] = old
            elif old is MISSING: delattr(holder, name)
            else: setattr(holder, name, old)

def run(*on):
    """run_overlay over the 80 made-up seconds with these switches on: what was on screen at every look, and when."""
    switches(*on); del LINES[:]; del ERRORS[:]
    O.HEALTH.stopped.clear(); O.HEALTH.last.clear(); O.HEALTH.errors.clear()
    try: os.remove(O.SETTINGS)                               # (a dragged spot from the run before)
    except OSError: pass
    world = types.SimpleNamespace(clock=Clock(), ticks=[], pending=None, windows={}, published=[], front=[500], shadows=[0],
                                  font=FakeFont())
    world.screen = Screen(world.clock)
    cfg = copy.deepcopy(C.load())
    cfg['overlay'].update(dps_window=True, hp_labels=True, dps_locked=True, dps_example=False, result_seconds=8)
    world.meter = types.SimpleNamespace(
        args=types.SimpleNamespace(cfg=cfg, player_log=None), hwnd=500, use_screen=False, capture_clean=lambda: False,
        fight=None, bar_seen=0.0, last_screen=None, frame_size=(0, 0), origin=(0, 0), bossrush_space=False,
        holosim_space=False, hit_at=0.0, stop=False, run=lambda: None)
    world.events = scenario(world)
    tk, tkfont = fake_tk(world)
    user32 = types.SimpleNamespace(GetForegroundWindow=lambda: world.front[0], GetParent=lambda hwnd: 0, IsIconic=lambda hwnd: 0,
                                   ShowWindow=lambda *a: 0, SetForegroundWindow=lambda hwnd: 0)
    shadow = O.with_shadow
    def counted(img, px): world.shadows[0] += 1; return shadow(img, px)
    p = Patch()
    try:
        p.module('tkinter', tk); p.module('tkinter.font', tkfont)
        p.module('PIL.ImageTk', types.ModuleType('PIL.ImageTk')); p.attr(PIL, 'ImageTk', sys.modules['PIL.ImageTk'])
        for name in ('top_hwnd', 'show', 'set_alpha', 'park', 'click_through'): p.attr(layered, name, getattr(world.screen, name))
        p.attr(wc.shared, 'publish_overlays', lambda owner, rects: world.published.append(world.clock.ms))   # (the app's shared memory)
        p.attr(gamefont.GameFont, 'load', staticmethod(lambda *a, **k: world.font))
        p.attr(O, 'console_out', lambda kind, data: None); p.attr(O, 'with_shadow', counted)
        p.attr(O, 'time', world.clock); p.attr(NW, 'time', world.clock)
        p.attr(O, 'threading', types.SimpleNamespace(Thread=NoThread))
        p.attr(O, 'ctypes', types.SimpleNamespace(windll=types.SimpleNamespace(user32=user32)))
        O.run_overlay(world.meter, False)
    finally: p.back()
    world.lines = list(LINES); world.errors = list(ERRORS) + ([O.HEALTH.last] if O.HEALTH.last or O.HEALTH.stopped else [])
    world.moves = {n: [(pic, at) for _, pic, at in c] for n, c in world.screen.changes.items()}
    return world

def differs(a, b):
    """Where two runs' looks first differ (None: nowhere)."""
    if len(a) != len(b): return f'{len(a)} looks against {len(b)}'
    for x, y in zip(a, b):
        if x != y: return f'at {x[0]} ms / {y[0]} ms: wait {x[1]} / {y[1]}, up {x[4]} / {y[4]}'
def up_at(world, ms): return next(t[4] for t in reversed(world.ticks) if t[0] <= ms)
def shows_in(world, name, a, b): return sum(1 for ms, n in world.screen.shows if n == name and a <= ms < b)
def late(world, ref, rhythm=False):
    """The most ms any change on screen came later than the same change in the reference run. A fight's numbers are
    redrawn 4 times a second from the look that first saw the fight, so their rhythm may sit a few ms off the
    reference's, as it does today from one fight to the next: those are measured apart (`rhythm`), the rest here."""
    def numbers(n, ms): return n in ('dps', 'pct', 'bar', 'total', 'break') and STARTS['fight'] + 100 < ms < 30_000
    return max(ms - ref.screen.changes[n][i][0] for n, c in world.screen.changes.items() for i, (ms, _, _) in enumerate(c)
               if numbers(n, ref.screen.changes[n][i][0]) == rhythm)
def redraws(world):
    """The ms between a fight's redraws while the numbers run (6 s to 12 s)."""
    at = sorted({ms for ms, n in world.screen.shows if n == 'dps' and 6100 < ms < 12_000})
    return {b - a for a, b in zip(at, at[1:])}
def first_after(world, name, ms): return next(t for t, _, _ in world.screen.changes[name] if t >= ms)

off = run()
check('off: the made-up 80 s ran without an error', off.errors, [])
LABELS = ('dps', 'pct', 'total', 'break')              # what's up while nothing changes (16 s to 20 s)
check('off: the made-up 80 s had everything in it',
      [up_at(off, ms) for ms in (500, 3000, 18_000, 21_000, 23_000, 25_500, 27_200, 35_000, 42_000, 50_000, 64_000, 70_100, 79_000)],
      [(), ('notice',), LABELS, ('dps', 'pct', 'bar', 'total', 'break'), ('alert', 'edge'), ('dps', 'pct', 'bar', 'total'),
       ('dps',), ('dps', 'notice'), ('dps',), (), ('dps',), ('alert', 'edge'), ()])
check('off: the same words in another colour are another picture', len({pic for ms, pic, at in off.screen.changes['total'] if 24_900 <= ms < 26_000}), 3)
check('off: the window followed the mouse', [at for ms, pic, at in off.screen.changes['dps'] if 28_560 <= ms < 28_720],
      [(off.grab[0] + 3 * i, off.grab[1] + 2 * i) for i in range(1, 11)])
check('off: a look every 15 ms, always', {t[1] for t in off.ticks}, {15})
check('off: no line from either switch', [l for l in off.lines if l.startswith(('overlay-label-cache', 'overlay-idle-poll'))], [])
check('off: with nothing changing, every window is shown again at each redraw (4 a second)',
      [shows_in(off, n, 16_400, 20_000) >= 12 for n in LABELS], [True] * 4)
check('off: the spots are told to the app each second', max(b - a for a, b in zip(off.published, off.published[1:])) <= 1020, True)

# overlay-label-cache: at every one of the 5,334 looks the same pictures (pixel for pixel) at the same spots, as opaque
lab = run('overlay-label-cache')
check('label cache: no error', lab.errors, [])
check('label cache: every look as with it off (when, the pixels, the spots, the opacity)', differs(lab.ticks, off.ticks), None)
check('label cache: every change on screen at the same ms', lab.screen.changes, off.screen.changes)
check('label cache: the spots told to the app at the same times', lab.published, off.published)
check('label cache: with nothing changing, a window is shown again only every 2 s',
      [shows_in(lab, n, 16_400, 20_000) <= 3 for n in LABELS], [True] * 4)
check('label cache: fewer windows shown, fewer labels drawn',
      (len(lab.screen.shows) < len(off.screen.shows) * 0.7, lab.shadows[0] < off.shadows[0] * 0.5), (True, True))
check('label cache: the DPS window still shown as it changes (the time runs on, 12 s to 16 s)',
      shows_in(lab, 'dps', 12_000, 16_000) >= 7, True)
told = [l for l in lab.lines if l.startswith('overlay-label-cache')]
check('label cache: one line in its minute, counts and milliseconds only',
      [bool(re.fullmatch(r'overlay-label-cache: labels \d+ drawn \(\d+ ms\), \d+ reused; windows \d+ shown \(\d+ ms\), '
                         r'\d+ left as they were \(last \d+ s\)', l)) for l in told], [True])
check('label cache: no line from the other switch', [l for l in lab.lines if l.startswith('overlay-idle-poll')], [])

# overlay-idle-poll: 50 ms only with nothing up and no fight; the same pictures in the same order, none over 50 ms later
idle = run('overlay-idle-poll')
check('idle poll: no error', idle.errors, [])
check('idle poll: 15 ms or 50 ms', {t[1] for t in idle.ticks}, {15, 50})
check('idle poll: 50 ms only with no fight and no window up', {(t[3], t[4]) for t in idle.ticks if t[1] == 50}, {(False, ())})
check('idle poll: 15 ms through the fight, the notices, the result, the example and the alert',
      [next(t[1] for t in reversed(idle.ticks) if t[0] <= ms) for ms in (3000, 9000, 23_000, 24_500, 27_200, 28_600, 31_000, 35_000, 42_000, 64_000, 72_000)],
      [15] * 11)
check('idle poll: 50 ms in between', [next(t[1] for t in reversed(idle.ticks) if t[0] <= ms) for ms in (500, 5500, 39_500, 58_000, 68_000, 77_000)], [50] * 6)
check('idle poll: far fewer looks with nothing up', (sum(1 for t in idle.ticks if t[1] == 50) >= 350, len(idle.ticks) < len(off.ticks) - 800), (True, True))
check('idle poll: the same pictures at the same spots in the same order, window by window', idle.moves, off.moves)
if idle.moves == off.moves:
    check('idle poll: nothing on screen more than 50 ms later than with it off', late(idle, off) <= 50, True)
    # in a fight the looks are 15 ms apart as ever; only where the 4-a-second rhythm starts can differ, so a number
    # lands at most one redraw from where it did
    check('idle poll: a fight\'s numbers are redrawn 4 times a second as ever', (redraws(idle), redraws(off)), ({255}, {255}))
    check('idle poll: and none of them more than one redraw later', late(idle, off, True) <= O.OVERLAY_REDRAW * 1000 + O.POLL_MS, True)
    check('idle poll: what begins with nothing up is on screen within 50 ms',
          [0 <= first_after(idle, win, STARTS[what]) - STARTS[what] <= 50 for what, win in
           (('notice', 'notice'), ('fight', 'dps'), ('fight', 'pct'), ('note', 'dps'), ('example', 'dps'), ('hit', 'alert'))], [True] * 6)
check('idle poll: the spots are still told to the app each second', max(b - a for a, b in zip(idle.published, idle.published[1:])) <= 1060, True)
told = [l for l in idle.lines if l.startswith('overlay-idle-poll')]
check('idle poll: one line in its minute, counts only',
      [bool(re.fullmatch(r'overlay-idle-poll: \d+ looks at 50 ms \(nothing up\), \d+ at 15 ms \(last \d+ s\)', l)) for l in told], [True])
check('idle poll: no line from the other switch', [l for l in idle.lines if l.startswith('overlay-label-cache')], [])

# both at once
both = run('overlay-label-cache', 'overlay-idle-poll')
check('both: no error', both.errors, [])
check('both: every look as with the idle poll alone (when, the pixels, the spots, the opacity)', differs(both.ticks, idle.ticks), None)
check('both: the same pictures at the same spots in the same order as with both off', both.moves, off.moves)
check('both: fewer windows shown', len(both.screen.shows) < len(off.screen.shows) * 0.7, True)
check('both: a line each', sorted(l.split(':')[0] for l in both.lines if l.startswith('overlay-')), ['overlay-idle-poll', 'overlay-label-cache'])

# off again after on: as before (nothing of a switch stays behind)
again = run()
check('off again: every look as the first time', differs(again.ticks, off.ticks), None)
check('off again: as many windows shown and labels drawn', (len(again.screen.shows), again.shadows[0]), (len(off.screen.shows), off.shadows[0]))
check('nothing of the stand-ins is left', (sys.modules.get('tkinter') is None or hasattr(sys.modules['tkinter'], 'TclError'),
                                          O.time is real_time, NW.time is real_time, 'publish_overlays' in vars(wc.shared),
                                          layered.show.__module__), (True, True, True, False, 'layered'))

# ---------------------------------------------------------------- the Leaderboards' sending: what stands in
# Made-up records and pictures in the test's own folder. submit's urllib is a stand-in with no way out, so nothing can
# leave this script; the sender's thread is never started (its loop is run here, on a made-up clock).
import urllib.error, urllib.parse, urllib.request
class Net:
    status = 200
    def __init__(self): self.online = False; self.sent = []; self.clock = None
    def urlopen(self, req, timeout=None):
        self.sent.append((self.clock.ms // 1000 if self.clock else None, self.online, req.full_url.rsplit('/', 1)[-1],
                          req.get_header('Content-type'), req.data))
        if not self.online: raise OSError('made-up: no connection')
        return self
    def read(self): return b'{"submissionId": "made-up"}'
    def __enter__(self): return self
    def __exit__(self, *a): return False
NET = Net()
real_urllib = submit.urllib
submit.urllib = types.SimpleNamespace(error=urllib.error, parse=urllib.parse,
                                      request=types.SimpleNamespace(Request=urllib.request.Request, urlopen=NET.urlopen))

ENC = {}                                                # how often each encoder ran
REAL = {n: getattr(submit, n) for n in ('_crop_bytes', '_evidence_bytes', '_full_bytes', '_party_bytes')}
def counted(n):
    def encode(*a): ENC[n] = ENC.get(n, 0) + 1; return REAL[n](*a)
    return encode
for n in REAL: setattr(submit, n, counted(n))

def full(rel): return os.path.join(submit.HERE, rel)
def picture(rel, seed, size=(1920, 1080)):
    """A made-up screenshot, the same for the same seed."""
    os.makedirs(os.path.dirname(full(rel)), exist_ok=True)
    rnd = random.Random(seed); im = Image.new('RGB', size, (20, 26, 44)); d = ImageDraw.Draw(im)
    for _ in range(60):
        x, y = rnd.randrange(size[0]), rnd.randrange(size[1])
        d.rectangle((x, y, x + rnd.randrange(20, 400), y + rnd.randrange(10, 200)), fill=tuple(rnd.randrange(256) for _ in range(3)))
    im.save(full(rel)); return rel

def sender():
    """The sender as its start leaves it, without the start (no thread, no settings read, signed in with a made-up token)."""
    sub = submit.Submitter.__new__(submit.Submitter)
    sub.stop = False; sub.wake = None
    sub.account = {'api_url': 'http://test.invalid', 'token': 'made-up', 'screenshots': True, 'link_character': True, 'submit': True}
    sub.config = sub.account; sub.config_stamp = sub._stamp(); sub.linked = sub.dispute = sub.last_result = sub.on_sent = None
    return sub

def parts(sent):
    """What went (or would have): its parts by name, and the whole body with its random boundary taken out."""
    ctype, body = sent[3], sent[4]
    if 'boundary=' not in ctype: return {'json': body}, body
    edge = ctype.split('boundary=')[1].encode(); out = {}
    for p in body.split(b'--' + edge)[1:-1]:
        head, _, data = p.partition(b'\r\n\r\n'); out[re.search(rb'name="(\w+)"', head).group(1).decode()] = data[:-2]
    return out, body.replace(edge, b'-')
def tries(sub, item, n=1):
    """Send it n times with no connection: what would have gone, each time."""
    out = []
    for _ in range(n):
        try: sub.send(item); out.append('sent')
        except OSError: out.append(parts(NET.sent[-1]))
    return out

def run_item():        # a run with its result, Battle Data and Party screens (cut to the parts read, and whole)
    return {'endpoint': 'submissions', 'payload': {'kind': 'interlink_run', 'clientRunId': 'made-up-run-1', 'score': 123456, 'extra': {'made': 'up'}},
            'evidence': picture('logs/proofs/a_result.png', 1), 'battle': picture('logs/proofs/a_battle.png', 2),
            'party': picture('logs/proofs/a_party.png', 3, (2800, 1575)), 'gizmos': None}
def sim_item():        # a kind without cuts, with two Gizmo screens
    return {'endpoint': 'submissions', 'payload': {'kind': 'sim_run', 'clientRunId': 'made-up-run-2', 'floor': 7},
            'evidence': picture('logs/proofs/b_result.png', 4, (1600, 900)), 'battle': None, 'party': None,
            'gizmos': [picture('logs/proofs/b_gizmo1.png', 5), picture('logs/proofs/b_gizmo2.png', 6, (2560, 1440))]}

# ---------------------------------------------------------------- proof-encode-once
sub = sender(); A, B = run_item(), sim_item(); NET.online = False
switches(); ENC.clear()
offA, offB = tries(sub, A, 2), tries(sub, B, 2)
check('off: every try encodes every picture again', ENC, {'_crop_bytes': 4, '_full_bytes': 4 + 6, '_party_bytes': 2, '_evidence_bytes': 2})
check('off: nothing is kept', (submit.PROOFS.kept, submit.PROOFS.size), ({}, 0))
check('off: what goes up', (sorted(offA[0][0]), sorted(offB[0][0])),
      (['battle', 'battle_full', 'evidence', 'evidence_full', 'party', 'submission'], ['evidence', 'evidence_full', 'gizmo1', 'gizmo2', 'submission']))
check('off: the same file encodes to the same bytes, try after try', (offA[0] == offA[1], offB[0] == offB[1]), (True, True))

switches('proof-encode-once'); ENC.clear()
onA, onB = tries(sub, A, 3), tries(sub, B, 3)
check('encode once: each picture encoded once for three tries', ENC, {'_crop_bytes': 2, '_full_bytes': 2 + 3, '_party_bytes': 1, '_evidence_bytes': 1})
check('encode once: every try sends byte for byte what goes up with it off',
      ([t == offA[0] for t in onA], [t == offB[0] for t in onB]), ([True] * 3, [True] * 3))
cuts = submit.PROOF_BOXES['interlink_run']
fresh = {'evidence': REAL['_crop_bytes'](full(A['evidence']), cuts['evidence']), 'evidence_full': REAL['_full_bytes'](full(A['evidence'])),
         'battle': REAL['_crop_bytes'](full(A['battle']), cuts['battle']), 'battle_full': REAL['_full_bytes'](full(A['battle'])),
         'party': REAL['_party_bytes'](full(A['party']))}
check('encode once: each picture sent again is what a fresh encode of its file gives',
      {k: onA[2][0][k] == v for k, v in fresh.items()}, dict.fromkeys(fresh, True))
check('encode once: 9 pictures kept, their bytes counted',
      (len(submit.PROOFS.kept), submit.PROOFS.size), (9, sum(len(v) for v in submit.PROOFS.kept.values())))

picture(A['evidence'], 11); st = os.stat(full(A['evidence']))                 # the result screen is taken again
os.utime(full(A['evidence']), ns=(st.st_atime_ns, st.st_mtime_ns + 2_000_000_000))
ENC.clear(); again = tries(sub, A)[0][0]
check('encode once: a screenshot written again is encoded again, the others are not', ENC, {'_crop_bytes': 1, '_full_bytes': 1})
check('encode once: and the new picture is the one that goes',
      (again['evidence'] == REAL['_crop_bytes'](full(A['evidence']), cuts['evidence']), again['evidence'] != onA[0][0]['evidence'],
       again['evidence_full'] == REAL['_full_bytes'](full(A['evidence'])), again['battle'] == onA[0][0]['battle']), (True, True, True, True))

os.remove(full(B['gizmos'][0])); ENC.clear(); gone = tries(sub, B)[0]         # a proof that's no longer there
check('encode once: a missing picture is left out, as with it off', (sorted(gone[0]), ENC), (['evidence', 'evidence_full', 'gizmo2', 'submission'], {'_full_bytes': 1}))

sub.drop_proof(A)                                                              # sent or set aside
check('encode once: a record done with takes its pictures along (the older copy of the one written again too)',
      (len(submit.PROOFS.kept), all('b_' in os.path.basename(k[1]) for k in submit.PROOFS.kept)), (4, True))
check('the screenshots of a record done with are removed, as ever', [os.path.exists(full(A[k])) for k in ('evidence', 'battle', 'party')], [False] * 3)
sub.drop_proof(B)
check('encode once: nothing kept once every record is done with', (submit.PROOFS.kept, submit.PROOFS.size), ({}, 0))

A, B = run_item(), sim_item(); os.remove(full(B['gizmos'][0])); tries(sub, A); tries(sub, B)
check('encode once: kept for the next try', len(submit.PROOFS.kept), 5 + 3)
switches(); ENC.clear(); offgone = tries(sub, B)[0]
check('turned off: nothing stays kept, and every picture is encoded', (submit.PROOFS.kept, submit.PROOFS.size, ENC), ({}, 0, {'_evidence_bytes': 1, '_full_bytes': 3}))
check('a missing picture: the same goes up either way', gone == offgone, True)

# the bounds, on a store of its own
small = []
for i in range(6):
    small.append(os.path.join(TMP, f'small{i}.bin'))
    with open(small[-1], 'wb') as f: f.write(bytes([i]) * 5)
calls = []
def mk(i, n=10): return lambda: (calls.append(i), bytes([i]) * n)[1]
def names(P): return [os.path.basename(k[1]) for k in P.kept]
P = submit.Proofs(); P.MAX_N = 3
for i in range(3): P.get('full', small[i], mk(i))
got = P.get('full', small[0], mk(0)); P.get('full', small[3], mk(3))
check('kept: at most MAX_N; the one just sent again stays, the oldest goes', (names(P), calls, got, P.size), (['small2.bin', 'small0.bin', 'small3.bin'], [0, 1, 2, 3], b'\0' * 10, 30))
P.get(('crop', (0.0, 0.0, 1.0, 1.0)), small[3], mk(4))
check('kept: the same file cut another way is another picture', (calls[-1], len(P.kept)), (4, 3))
P = submit.Proofs(); P.MAX_BYTES = 25
for i in range(3): P.get('full', small[i], mk(i))
check('kept: at most MAX_BYTES', (names(P), P.size), (['small1.bin', 'small2.bin'], 20))
check('kept: one too large for the store alone is sent and not kept', (P.get('full', small[4], mk(4, 30)), P.kept, P.size), (b'\4' * 30, {}, 0))
check('kept: only bytes', (P.get('full', small[5], lambda: None), P.kept), (None, {}))
def rewrite():
    with open(small[5], 'wb') as f: f.write(b'written again, longer')
    return b'old'
check('kept: not what was encoded from a file written again meanwhile', (P.get('full', small[5], rewrite), P.kept), (b'old', {}))
check('kept: no file, no keeping (it fails as it would have)', (P.get('full', os.path.join(TMP, 'none.bin'), lambda: 'made'), P.kept, P.n['encoded']), ('made', {}, 3 + 1 + 2))
check('the store\'s own bounds', (submit.Proofs.MAX_N, submit.Proofs.MAX_BYTES), (48, 24_000_000))

P = submit.Proofs(); P.get('full', small[0], mk(0)); P.get('full', small[0], mk(0)); P.get('full', small[1], mk(1))
del LINES[:]; P.tell(30.0); P.tell(61.0); P.tell(200.0)
check('encode once: one line a minute, counts, milliseconds and sizes only',
      [re.sub(r'\(\d+ ms\)', '(N ms)', l) for l in LINES], ['proof-encode-once: 2 proof pictures encoded (N ms), 1 sent again as they were; 2 kept (0 KB)'])

# ---------------------------------------------------------------- queue-wait-fight: whether a pass waits
R1, R2 = 'interlink_run__made-up-run-1.json', 'interlink_run__made-up-run-9.json'
def payload(n): return {'kind': 'interlink_run', 'clientRunId': f'made-up-run-{n}', 'score': 1000 * n, 'server': 'made-up'}
def empty_queue():
    os.makedirs(submit.QUEUE, exist_ok=True)
    for n in os.listdir(submit.QUEUE): os.remove(os.path.join(submit.QUEUE, n))
def waiting(): return sorted(n for n in os.listdir(submit.QUEUE) if n.endswith('.json'))
WAITED = 'queue-wait-fight: a retry waited '
def held(sub, woke=False, now=5000.0, since=None, **seen):
    """One look of the sender at whether to wait, from a given state; the line it wrote, if any."""
    sub.held_since = since; del LINES[:]
    if seen: sub.fight_seen = seen['seen']
    return sub.hold_retry(woke, now), sub.held_since, [l[len(WAITED):] for l in LINES if l.startswith(WAITED)]

empty_queue(); sub = sender(); submit.write_json(os.path.join(submit.QUEUE, R1), {'endpoint': 'submissions', 'payload': payload(1)})
switches('queue-wait-fight'); FIGHT = (5000.0, True)
check('wait: no word from the meter, no waiting', held(sub), (False, None, []))
check('wait: the first pass after a start never waits', held(sub, seen=FIGHT), (False, None, []))
sub.tried = sub.queued()
check('wait: a retry with a fight on waits', held(sub), (True, 5000.0, []))
check('wait: and goes on waiting, from when it began', held(sub, now=5002.0, since=5000.0, seen=(5002.0, True)), (True, 5000.0, []))
check('wait: the fight over, it goes', held(sub, now=5031.0, since=5000.0, seen=(5031.0, False)), (False, None, ['31 s for a fight (the fight was over)']))
check('wait: the meter\'s word older than 5 s is not believed (the meter stopped)',
      (held(sub, now=5004.9, seen=(5000.0, True))[0], held(sub, now=5005.0, since=5000.0, seen=(5000.0, True))), (True, (False, None, ['5 s for a fight (the fight was over)'])))
check('wait: nor a word from the future (the clock was set back)', held(sub, now=4990.0, seen=FIGHT), (False, None, []))
check('wait: woken for a new record, it goes at once', held(sub, woke=True, since=4990.0, seen=FIGHT), (False, None, ['10 s for a fight (a new record came)']))
check('wait: woken with nothing waited for, it just goes', held(sub, woke=True), (False, None, []))
check('wait: never longer than HOLD_MAX_S', (held(sub, since=5000.0 - submit.HOLD_MAX_S + 1)[0], held(sub, since=5000.0 - submit.HOLD_MAX_S)),
      (True, (False, None, ['300 s for a fight (the longest it waits)'])))
check('wait: a clock set back ends it', held(sub, since=5010.0), (False, None, ['0 s for a fight (the longest it waits)']))
submit.write_json(os.path.join(submit.QUEUE, R2), {'endpoint': 'submissions', 'payload': payload(9)})
check('wait: a record the last pass did not see goes at once (even if its wake was missed)', held(sub, since=4990.0), (False, None, ['10 s for a fight (a new record came)']))
sub.tried = sub.queued(); check('wait: once it had its turn, the retry waits again', held(sub)[0], True)
st = os.stat(os.path.join(submit.QUEUE, R1)); os.utime(os.path.join(submit.QUEUE, R1), ns=(st.st_atime_ns, st.st_mtime_ns + 1_000_000_000))
check('wait: a record written again goes at once', held(sub), (False, None, []))
sub.tried = sub.queued(); empty_queue()
check('wait: nothing waiting, nothing held', held(sub), (False, None, []))
check('wait: the bounds', (submit.HOLD_MAX_S, submit.HOLD_LOOK_S, submit.FIGHT_STALE_S), (300, 2, 5))
switches(); sub.tried = {R1: 1}
check('off: never held, and nothing of the switch stays', (held(sub, since=4990.0), sub.tried), ((False, None, []), None))

# ---------------------------------------------------------------- queue-wait-fight: the sender's loop over 15 made-up minutes
class Done(Exception): pass
class Wake:
    """The sender's wait on a made-up clock: time only passes here, and what's planned happens on the way (the meter's
    word on the fight each second, a record coming in, the connection coming back)."""
    def __init__(self, clock, plan, end): self.clock, self.plan, self.end = clock, sorted(plan, key=lambda e: e[0]), end; self.flag = False; self.looks = []
    def set(self): self.flag = True
    def clear(self): self.flag = False
    def wait(self, delay):
        until = self.clock.ms + round(delay * 1000); self.looks.append((self.clock.ms // 1000, delay, waiting()))
        while not self.flag and self.plan and self.plan[0][0] <= until:
            at, fn = self.plan.pop(0); self.clock.ms = max(self.clock.ms, at); fn()
        if not self.flag: self.clock.ms = until
        if self.clock.ms >= self.end: raise Done
        return self.flag

def fights(ms): return 2000 <= ms < 130_000 or 150_000 <= ms < 600_000      # two fights, the second 7.5 minutes long
def queue_run(*on, end=900, online_at=400, keep=False, fight=fights):
    """The sender's own loop: a record with a picture waits from the start, with no connection until `online_at` s;
    another record comes at 200 s. The meter's tick (the real one) tells the sender each second whether a fight is on."""
    switches(*on); del LINES[:]; ENC.clear(); submit.PROOFS = submit.Proofs()
    clock = Clock(); NET.clock, NET.sent, NET.online = clock, [], online_at <= 0
    sub = sender()
    if not keep:
        empty_queue(); sub.wake = types.SimpleNamespace(set=lambda: None)
        sub.enqueue(payload(1), evidence_path=picture('logs/proofs/q_result.png', 21))
    meter = types.SimpleNamespace(submitter=sub, fight=None, args=types.SimpleNamespace(replay=0), uid=None)
    def word(): meter.fight = {'made': 'up'} if fight(clock.ms) else None; leaderboards.LeaderboardsMixin.presence_tick(meter, clock.time())
    def online(): NET.online = True
    plan = [(s * 1000, word) for s in range(1, end + 1)] + [(online_at * 1000, online)]
    if not keep: plan.append((200_000, lambda: sub.enqueue(payload(9))))
    sub.wake = Wake(clock, plan, end * 1000)
    old = submit.time, leaderboards.time
    submit.time = leaderboards.time = clock
    try: sub.run()
    except Done: pass
    finally: submit.time, leaderboards.time = old; NET.clock = None
    return types.SimpleNamespace(sub=sub, tries=[(at, name, ok) for at, ok, name, _, _ in NET.sent], lines=list(LINES), enc=dict(ENC),
                                 went=[parts(s)[1] for s in NET.sent if s[1]], looks=sub.wake.looks, left=waiting(), kept=dict(submit.PROOFS.kept))
def fight_lines(q): return [l for l in q.lines if l.startswith('queue-wait-fight')]
def proof_lines(q): return [re.sub(r'\(\d+ (ms|KB)\)', '(N)', l) for l in q.lines if l.startswith('proof-encode-once')]

qoff = queue_run()
MINUTES = [5, 65, 125, 185, 200, 260, 320, 380, 440]        # today: 5 s after the start, every minute, and when a record comes (200 s)
check('off: a try each minute, fight or not, until it goes through',
      qoff.tries, [(s, 'submissions', False) for s in MINUTES[:-1]] + [(440, 'submissions', True)] * 2)
check('off: both records went, nothing waits', (len(qoff.went), qoff.left), (2, []))
check('off: the picture encoded again for every try', qoff.enc, {'_crop_bytes': 9, '_full_bytes': 9})
check('off: the meter says nothing to the sender, the sender remembers nothing, no line', (hasattr(qoff.sub, 'fight_seen'), qoff.sub.tried, fight_lines(qoff) + proof_lines(qoff)), (False, None, []))
check('off: the looks are 60 s apart with records waiting, 300 s with none', [d for _, d, _ in qoff.looks], [5] + [60] * 8 + [300] * 2)

qon = queue_run('queue-wait-fight')
check('wait: the first pass as ever (5 s), the retries after each fight, a new record at once, the ceiling',
      qon.tries, [(5, 'submissions', False), (131, 'submissions', False), (200, 'submissions', False), (560, 'submissions', True), (560, 'submissions', True)])
check('wait: one line per wait, seconds only', fight_lines(qon),
      ['queue-wait-fight: a retry waited 66 s for a fight (the fight was over)', 'queue-wait-fight: a retry waited 9 s for a fight (a new record came)',
       'queue-wait-fight: a retry waited 300 s for a fight (the longest it waits)'])
check('wait: both records went, byte for byte what goes with it off, nothing waits', (qon.went == qoff.went, len(qon.went), qon.left), (True, 2, []))
check('wait: every record stays in its file until it has gone',
      (all(R1 in names for at, _, names in qon.looks if at < 560), all(R2 in names for at, _, names in qon.looks if 200 <= at < 560),
       [names for at, _, names in qon.looks if at >= 560]), (True, True, [[], []]))
check('wait: while it waits it looks every 2 s', {d for at, d, _ in qon.looks if 65 <= at < 131 or 260 <= at < 560}, {2})
check('wait: the meter told the sender', qon.sub.fight_seen[1], False)
check('wait: no try while a fight was on, but the first, the new record\'s and the ceiling\'s', [at for at, _, _ in qon.tries if fights(at * 1000)], [5, 200, 560, 560])

qenc = queue_run('proof-encode-once')
check('encode once: tries at the same times as with it off', qenc.tries, qoff.tries)
check('encode once: the picture encoded once for nine tries', qenc.enc, {'_crop_bytes': 1, '_full_bytes': 1})
check('encode once: what went is byte for byte what goes with it off', (qenc.went == qoff.went, len(qenc.went)), (True, 2))
check('encode once: nothing kept once it went', (qenc.kept, qenc.left), ({}, []))
check('encode once: its lines, a minute apart at the least', proof_lines(qenc),
      ['proof-encode-once: 2 proof pictures encoded (N), 0 sent again as they were; 2 kept (N)']
      + ['proof-encode-once: 0 proof pictures encoded (N), 2 sent again as they were; 2 kept (N)'] * 3
      + ['proof-encode-once: 0 proof pictures encoded (N), 4 sent again as they were; 2 kept (N)']
      + ['proof-encode-once: 0 proof pictures encoded (N), 2 sent again as they were; 2 kept (N)'] * 2
      + ['proof-encode-once: 0 proof pictures encoded (N), 2 sent again as they were; 0 kept (N)'])

qboth = queue_run('queue-wait-fight', 'proof-encode-once')
check('both: tries as with the wait alone, one encode, the same bytes', (qboth.tries == qon.tries, qboth.enc, qboth.went == qoff.went, qboth.left),
      (True, {'_crop_bytes': 1, '_full_bytes': 1}, True, []))

# the app closed while a retry waits: the record is in its file, and the next start sends it in its first pass, fight or not
qcut = queue_run('queue-wait-fight', end=100, online_at=10**6)
check('closed mid-wait: the record is still waiting', (qcut.tries, qcut.left, R1 in qcut.looks[-1][2]), ([(5, 'submissions', False)], [R1], True))
qnext = queue_run('queue-wait-fight', end=100, online_at=0, keep=True, fight=lambda ms: True)
check('started again in a fight: it goes in the first pass', (qnext.tries, qnext.left), ([(5, 'submissions', True)], []))
# fights back to back for the whole 15 minutes with no connection: a try at each ceiling, the record never dropped
qlong = queue_run('queue-wait-fight', online_at=10**6, fight=lambda ms: True)
check('fights without end: the first pass, the new record, then a try every 360 s', [at for at, _, _ in qlong.tries], [5, 200, 560])
check('fights without end: both records still waiting', qlong.left, [R1, R2])
check('no plug-in error on the way', (O.HEALTH.failed('leaderboards'), O.HEALTH.failed('dps')), (False, False))

D.use(None); empty_queue(); submit.urllib = real_urllib; submit.PROOFS = submit.Proofs()
for n in REAL: setattr(submit, n, REAL[n])
if fails: print('\n'.join(fails)); sys.exit(1)
print('dev overlay + submit switches: ok')
