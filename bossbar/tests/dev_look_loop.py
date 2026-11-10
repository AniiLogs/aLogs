"""Beta Menu > Dev: the reading loop's switches, each one off (today's behaviour) and on.

    python tests/dev_look_loop.py

- fill-fast: the same answer as measure_fill_any, value for value, on thousands of made-up bars (random fills, colours
  and noise; empty, full, no bar at all, our own cover on the bar, specks, bars at the picture's edge) - and the old way
  for every picture or bar that isn't plain;
- fight-skip-unchanged: read_counter's answer every time, worked out again only when the white shapes changed;
- top-sig-direct: the same 48x27 bytes as the old way at every frame size, and the old way for good once they differ;
- proof-off-thread: the same files with the same bytes, a full queue saves in the look, an error is one log line;
- proof-writer-low: the writer's thread at the lowest priority only with its own switch;
- quiet-top-only: after a sent Egg Heist match, whole frames only on the looks a screen read is made from;
- busy-half-pace: 2 screen reads and 10 looks a second while nothing can't wait, today's pace for everything that can't;
- area-keep-short: a whole frame stays asked for 0.3 s, not 2 s;
- helpers-below-normal: new OCR helpers start below normal priority, running ones follow the switch;
- tabbed-out-rest: tabbed out with no fight, the whole-frame bar search runs twice a second.
Made-up pictures only; readers, the capture and the helpers are stubbed. Exit code 1 on any failure.
"""
import ctypes, json, logging, os, random, re, subprocess, sys, tempfile, threading, time
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
TMP = tempfile.mkdtemp(prefix='alogs_look_')
os.environ['ALOGS_DATA'] = TMP
os.environ.pop('ALOGS_DEV_ON', None)
from PIL import Image, ImageChops, ImageDraw
import devflags as D
import core, live, reader as R, wincapture as wc, xn_digits as X
from live import Meter
logging.getLogger('meter').addHandler(logging.NullHandler()); logging.getLogger('meter').propagate = False
said = []
class Hear(logging.Handler):
    def emit(self, record): said.append(record.getMessage())
logging.getLogger('meter').addHandler(Hear()); logging.getLogger('meter').setLevel(logging.INFO)

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

FLAGS = os.path.join(TMP, 'dev-on.json')
def switches(*on):
    """As the app writes it: whole, then the meter looks."""
    with open(FLAGS + '.tmp', 'w', encoding='utf-8') as f: json.dump({'on': list(on)}, f)
    os.replace(FLAGS + '.tmp', FLAGS)
    D.use(FLAGS)

def source(name):
    with open(os.path.join(HERE, name), encoding='utf-8') as f: return f.read()

rng = random.Random(20261110)
def colour(lo=0, hi=255): return tuple(rng.randint(lo, hi) for _ in range(3))
def noise(size, amp):
    """A picture of random levels 0..amp."""
    return Image.frombytes('RGB', size, rng.randbytes(size[0] * size[1] * 3)).point(lambda v: v * (amp + 1) // 256)
def noisy(im, amp):
    if not amp: return im
    return (ImageChops.add if rng.random() < 0.5 else ImageChops.subtract)(im, noise(im.size, amp))
def files(*parts):
    d = os.path.join(TMP, 'logs', *parts)
    return sorted(os.listdir(d)) if os.path.isdir(d) else []
def content(*parts):
    with open(os.path.join(TMP, 'logs', *parts), 'rb') as f: return f.read()
D.use(None)

# ---------------------------------------------------------------- fill-fast
def made_up_bar(kind):
    """A picture with a bar on it (or none), and where the bar is said to be: (picture, bar, jump)."""
    W = rng.choice((64, 120, 257, 400, 640)); H = rng.randint(14, 48)
    t = rng.randint(2, 14); q = max(1, t // 4); yc = rng.randint(q, H - 1 - q)
    left = rng.randint(0, W // 4)
    right = rng.choice((rng.randint(left + 12, W - 3), W, W + 20)) if kind != 'edge' else W + rng.randint(0, 9)
    n = min(right, W - 3) - left
    im = Image.new('RGB', (W, H), colour()); d = ImageDraw.Draw(im)
    rows = (yc - t // 2, yc + t // 2)
    if kind != 'none':
        fill = {'empty': 0.0, 'full': 1.0}.get(kind, rng.random())
        end = left + int(n * fill)
        d.rectangle((left, rows[0], right, rows[1]), fill=colour())             # the track
        if end > left: d.rectangle((left, rows[0], end - 1, rows[1]), fill=colour())
        if kind == 'flash':                                                      # a white flash, then a ghost, after the fill
            a = end + rng.randint(0, 12); b = a + rng.randint(0, 20)
            d.rectangle((end, rows[0], a, rows[1]), fill=colour(230, 255)); d.rectangle((a, rows[0], b, rows[1]), fill=colour())
        if kind == 'speck':                                                      # one- and two-pixel columns of something else
            for _ in range(rng.randint(1, 6)):
                x = rng.randint(left, left + n); d.rectangle((x, 0, x + rng.randint(0, 1), H - 1), fill=colour())
        if kind == 'rows':                                                       # the three rows disagree with each other
            for y in range(H):
                if rng.random() < 0.5: d.line((left, y, right, y), fill=colour())
        if kind == 'stripes':                                                    # rows of the same brightness, other colours
            off = Image.new('RGB', (W, H)); od = ImageDraw.Draw(off)
            for y in range(H): od.line((0, y, W, y), fill=((0, 2, 4), (2, 4, 0), (4, 0, 2))[y % 3])
            im = ImageChops.add(im.point(lambda v: min(v, 250)), off)
    im = noisy(im, rng.choice((0, 0, 1, 1, 2, 5, 30, 80)))
    if kind == 'cover':                                                          # our own HP labels over part of the bar
        d = ImageDraw.Draw(im)
        for _ in range(rng.randint(1, 3)):
            x = rng.randint(left - 5, left + n); d.rectangle((x, 0, x + rng.randint(0, n), H - 1), fill=R.COVER)
    return im, {'yc': yc, 'thickness': t, 'track_left': left, 'track_right': right}, rng.choice((70, 70, 70, 40, 100))

def fill_of(case):
    try: return repr(R.measure_fill_any(*case))
    except Exception as e: return type(e).__name__

plain = [made_up_bar(kind) for kind in ('plain', 'empty', 'full', 'none', 'flash', 'speck', 'rows', 'stripes', 'cover', 'edge')
         for _ in range(400)]
for W in (1280, 3840):                                                           # bars as long as the real ones
    for _ in range(20):
        im = noisy(Image.new('RGB', (W, 40), colour()), rng.choice((0, 2, 30)))
        d = ImageDraw.Draw(im); left = W // 3; right = W - W // 3; end = left + int((right - left) * rng.random())
        d.rectangle((left, 14, right, 26), fill=colour()); d.rectangle((left, 14, end, 26), fill=colour())
        plain.append((noisy(im, rng.choice((0, 1, 3))), {'yc': 20, 'thickness': 12, 'track_left': left, 'track_right': right}, 70))
# the three rows of one brightness and three colours, to the pixel: the middle row's is the median, as sorted() leaves it
im = Image.new('RGB', (60, 9))
for y, c in enumerate(((10, 20, 30), (30, 20, 10), (20, 20, 20)) * 3): ImageDraw.Draw(im).line((0, y, 59, y), fill=c)
plain.append((im, {'yc': 4, 'thickness': 4, 'track_left': 2, 'track_right': 50}, 70))
odd = []                                                                         # not plain: the old way does these
for _ in range(60):
    im, bar, jump = made_up_bar(rng.choice(('plain', 'flash', 'cover')))
    how = rng.choice(('rgba', 'float', 'tiny', 'below', 'above', 'before'))
    if how == 'rgba': im = im.convert('RGBA')
    elif how == 'float': bar = {**bar, rng.choice(('yc', 'track_left', 'track_right')): bar['yc'] + 0.0}
    elif how == 'tiny': bar = {**bar, 'track_right': bar['track_left'] + rng.randint(0, 5)}
    elif how == 'below': bar = {**bar, 'yc': im.height + rng.randint(-1, 3)}
    elif how == 'above': bar = {**bar, 'yc': 0, 'thickness': 8}
    else: bar = {**bar, 'track_left': -rng.randint(1, 4)}
    odd.append((im, bar, jump))
before = dict(R.DEV)
old = [fill_of(c) for c in plain + odd]
check('fill-fast off: nothing of it runs', R.DEV, before)
switches('fill-fast')
new = [fill_of(c) for c in plain + odd]
D.use(None)
different = [i for i, (a, b) in enumerate(zip(old, new)) if a != b]
check(f'fill-fast: the same answer on {len(old)} made-up bars', [(i, old[i], new[i]) for i in different[:3]], [])
check('fill-fast: every plain bar went the fast way', R.DEV['fill_fast'] - before['fill_fast'], len(plain))
check('fill-fast: the others went the old way', R.DEV['fill_old'] - before['fill_old'], len(odd))
check('fill-fast: the made-up bars read as bars', len({o for o in old[:len(plain)]}) > len(plain) // 2, True)
check('fill-fast: and some have nothing to read', sum(not o.startswith('(') for o in old) > 0, True)
check('fill-fast: guarded', "if D.on('fill-fast'):" in source('reader.py'), True)

# ---------------------------------------------------------------- fight-skip-unchanged
SEG = {'0': 'abcdef', '1': 'bc', '2': 'abged', '3': 'abgcd', '4': 'fgbc', '5': 'afgcd', '6': 'afgedc', '7': 'abc', '8': 'abcdefg', '9': 'abfgcd'}
WHITE = (255, 255, 255)
def counter(text, k=1, back=None, pad=4):
    """A counter in block digits ('x12'), white on a dark made-up background; k: its size (digits 20*k px tall)."""
    w, h, s, gap = 12 * k, 20 * k, 3 * k, 4 * k
    width = pad * k + 9 * k + gap + sum((s if c == '1' else w) + gap for c in text[1:]) + pad * k
    im = noise((width, h + 2 * pad * k), 120) if back is None else Image.new('RGB', (width, h + 2 * pad * k), back)
    d = ImageDraw.Draw(im); x = pad * k; y = pad * k
    d.rectangle((x + 3 * k, y + 9 * k, x + 6 * k - 1, y + h - 1), fill=WHITE)    # the 'x': a small cross, shorter than a digit
    d.rectangle((x, y + 13 * k, x + 9 * k - 1, y + 16 * k - 1), fill=WHITE); x += 9 * k + gap
    for c in text[1:]:
        cw = s if c == '1' else w; m = y + (h - s) // 2
        seg = {'a': (x, y, x + cw - 1, y + s - 1), 'g': (x, m, x + cw - 1, m + s - 1), 'd': (x, y + h - s, x + cw - 1, y + h - 1),
               'f': (x, y, x + s - 1, m + s - 1), 'b': (x + cw - s, y, x + cw - 1, m + s - 1),
               'e': (x, m, x + s - 1, y + h - 1), 'c': (x + cw - s, m, x + cw - 1, y + h - 1)}
        for name in SEG[c]: d.rectangle(seg[name], fill=WHITE)
        x += cw + gap
    return im

# the mask is what it always was: near-white and not coloured, pixel by pixel
for _ in range(30):
    im = Image.frombytes('RGB', (37, 23), rng.randbytes(37 * 23 * 3)).point(lambda v: 120 + v * 136 // 256)
    px = im.load()
    want = [[int(min(px[x, y]) > 165 and max(px[x, y]) - min(px[x, y]) < 60) for x in range(37)] for y in range(23)]
    if X.white_mask(im) != want: fails.append('the white mask changed'); break
TPL = {c: X.split_counter(counter('x' + c, 2))[0] for c in SEG}
check('made-up digits: ten shapes of 216 points', sorted(len(v) for v in TPL.values()), [216] * 10)
for n in range(1, 22):
    check(f'made-up digits: x{n} reads', X.read_counter(counter(f'x{n}', 2), TPL, top=21), n)

real_read = X.read_counter; reads = []
X.read_counter = lambda *a, **k: (reads.append(1), real_read(*a, **k))[1]
X.SAME.update(reads=0, same=0, last=None)
a = counter('x12', 2)
check('the first read is a read', (X.read_counter_same(a, TPL, 21), len(reads), X.SAME['same']), (12, 1, 0))
check('the very same box: the last answer', (X.read_counter_same(a, TPL, 21), len(reads), X.SAME['same']), (12, 1, 1))
b = counter('x12', 2)                                                            # the same digits over another background
check('another picture, the same white shapes', (a.tobytes() != b.tobytes(), X.mask_bytes(a) == X.mask_bytes(b)), (True, True))
check('...is the last answer too', (X.read_counter_same(b, TPL, 21), len(reads), X.SAME['same']), (12, 1, 2))
check('the counter changed: read again', (X.read_counter_same(counter('x11', 2), TPL, 21), len(reads)), (11, 2))
c = counter('x11', 2)
check('another top: read again', (X.read_counter_same(c, TPL, 5), len(reads)), (None, 3))
check('no answer is an answer too', (X.read_counter_same(c, TPL, 5), len(reads)), (None, 3))
check('other templates: read again', (X.read_counter_same(c, dict(TPL), 21), len(reads)), (11, 4))
check('another limit: read again', (X.read_counter_same(c, TPL, 21, 0.1), len(reads)), (real_read(c, TPL, 0.1, 21), 5))
n = X.SAME['reads']
check('without templates it is read_counter itself', X.read_counter_same(c, None, 21), real_read(c, None, top=21))
check('without a top too', X.read_counter_same(c, TPL, None), real_read(c, TPL))
check('...and nothing is remembered of those', X.SAME['reads'], n)
X.read_counter = real_read

# a fight's worth of boxes: the counter stays, changes, is hidden, small (720p boxes are enlarged first), cut off
X.SAME.update(reads=0, same=0, last=None)
tpl2 = dict(TPL); n = 21; wrong = []; answers = set()
for i in range(700):
    r = rng.random()
    if r < 0.25: n = rng.randint(1, 25)
    k = 1 if rng.random() < 0.15 else 2
    box = counter(f'x{n}', k, back=rng.choice((None, None, colour(0, 150))))
    if rng.random() < 0.05: box = noise((70, 44), 255)                           # no counter there
    if rng.random() < 0.05: box = box.crop((0, 0, box.width - rng.randint(1, 20), box.height))
    if rng.random() < 0.05: box = noisy(box, 90)
    tpl = tpl2 if rng.random() < 0.05 else TPL; top = 5 if rng.random() < 0.05 else 21
    want = X.read_counter(box, tpl, top=top); got = X.read_counter_same(box, tpl, top)
    answers.add(want)
    if got != want: wrong.append((i, n, k, got, want))
check('fight-skip-unchanged: read_counter\'s answer on 700 boxes in a row', wrong[:3], [])
check('...with numbers and no answers among them', (None in answers, len(answers) > 15), (True, True))
check('...many of them from the last read', (X.SAME['reads'], X.SAME['same'] > 150), (700, True))
check('fight-skip-unchanged: guarded', "if D.on('fight-skip-unchanged') else X.read_counter(box, self.xn_tpl, top=top)" in source('live.py'), True)

# ---------------------------------------------------------------- top-sig-direct
def frame(size):
    """A made-up frame: coarse blocks of colour with fine noise on them (so which point is picked matters)."""
    w, h = size
    small = Image.frombytes('RGB', (max(1, w // 16), max(1, h // 16)), rng.randbytes(max(1, w // 16) * max(1, h // 16) * 3))
    im = small.resize(size, Image.NEAREST)
    return ImageChops.add(im, noise((min(w, 256), min(h, 256)), 60).resize(size, Image.NEAREST) if w * h > 2_000_000 else noise(size, 60))
differ = []
for size in ((3840, 2160), (2560, 1440), (1920, 1080), (1280, 720), (1366, 768), (1600, 900), (3440, 1440), (2560, 1080),
             (1920, 1200), (1001, 777), (47, 30), (48, 27), (49, 28), (40, 300), (1, 1), (3, 500)):
    im = frame(size); top_h = max(1, int(size[1] * live.TOP_PART))
    for th, pic in ((top_h, im), (top_h, im.crop((0, 0, size[0], top_h))),               # a whole frame; the top part alone
                    (top_h, im.crop((0, 0, size[0], max(1, top_h // 2)))),               # a picture shorter than the top part
                    (max(1, top_h - 1), im), (top_h + 1, im), (size[1], im), (1, im)):
        if core.top_sig_direct(pic, th) != core.screen_sig(pic.crop((0, 0, pic.width, th))): differ.append((pic.size, th))
check('top-sig-direct: the same bytes as the old way, every size', differ, [])
check('top-sig-direct: 48x27 of them', len(core.top_sig_direct(frame((640, 360)), 151)), 48 * 27)
m = Meter.__new__(Meter); im = frame((640, 360)); want = core.screen_sig(im.crop((0, 0, 640, 151)))
real_direct = live.top_sig_direct; direct = []
live.top_sig_direct = lambda im, top_h: (direct.append(1), real_direct(im, top_h))[1]
check('the first look of a frame size: the old way, and the two compared', (m.top_sig_fast(im, 151, 100.0), len(direct), m.dev_n if hasattr(m, 'dev_n') else {}), (want, 1, {}))
check('the next looks: direct', ([m.top_sig_fast(im, 151, 100.0 + i) for i in range(1, 60)], m.dev_n['sig_direct']), ([want] * 59, 59))
direct.clear()
check('a minute on: compared again', (m.top_sig_fast(im, 151, 160.0), len(direct), m.dev_n['sig_direct'], m.sig_check['bad']), (want, 1, 59, False))
im2 = frame((320, 180)); want2 = core.screen_sig(im2.crop((0, 0, 320, 75)))
live.top_sig_direct = lambda im, top_h: b'\0' * 1296                              # as if the two ways came apart
n = len(said)
check('the two differ: the old way\'s answer', (m.top_sig_fast(im2, 75, 161.0), m.sig_check['bad']), (want2, True))
check('...said once, with sizes only', [s for s in said[n:]], ['dev top-sig-direct: the two ways differ at 320x180, top 75: the old way from here on'])
live.top_sig_direct = lambda im, top_h: 1 / 0                                    # ...and it's never asked again
check('...and the old way from then on, every size', (m.top_sig_fast(im, 151, 162.0), m.top_sig_fast(im2, 75, 163.0), m.dev_n['sig_direct']), (want, want2, 59))
live.top_sig_direct = real_direct
check('top-sig-direct: guarded', "self.top_sig_fast(im, top_h, now) if D.on('top-sig-direct') else screen_sig(im.crop((0, 0, im.width, top_h)))" in source('live.py'), True)

# ---------------------------------------------------------------- proof-off-thread
W0 = core.WRITER
def blocked():
    """The writer's thread kept busy until the gate opens (so what's queued waits)."""
    gate, started = threading.Event(), threading.Event()
    W0.put(lambda: (started.set(), gate.wait(20)))
    started.wait(10)
    return gate
shot = frame((640, 360)); black = Image.new('RGB', (640, 360))
ran = []
core.picture_job(lambda: ran.append(threading.current_thread().name))
check('off: a picture is saved where it is taken', ran, ['MainThread'])
core.keep_sample(shot, 'offpng'); core.keep_sample(shot.convert('RGBA'), 'offrgba'); core.keep_sample(shot, 'offjpg', gap=0)
check('off: the samples are on disk at once', [n.split('_')[0] + n[-4:] for n in files('samples')], ['offjpg.jpg', 'offpng.png', 'offrgba.png'])
check('off: no writer thread, nothing counted', (W0.thread, W0.n), (None, {'queued': 0, 'inline': 0, 'failed': 0, 'ms': 0.0, 'max': 0.0}))
switches('proof-off-thread')
gate = blocked()
core.keep_sample(shot, 'onpng'); mine = shot.copy(); core.keep_sample(mine, 'onjpg', gap=0)
mine.paste(black)                                                                # the look's frame is drawn over before it's written
check('on: the look goes on before the picture is written', [n for n in files('samples') if n.startswith('on')], [])
gate.set()
check('on: the writer gets through them', W0.flush(), True)
core.keep_sample(shot.convert('RGBA'), 'onrgba'); W0.flush()
names = {n.split('_')[0]: n for n in files('samples')}
check('on: the same file names', sorted(names), ['offjpg', 'offpng', 'offrgba', 'onjpg', 'onpng', 'onrgba'])
for kind in ('png', 'jpg', 'rgba'):
    check(f'on: the same bytes ({kind})', content('samples', names['on' + kind]) == content('samples', names['off' + kind]), True)
    check(f'on: the same name pattern ({kind})', re.sub(r'\d', '0', names['on' + kind][2:]), re.sub(r'\d', '0', names['off' + kind][3:]))
check('on: taken by the writer', (W0.n['queued'], W0.n['inline'], W0.n['failed'], W0.thread.name, W0.thread.daemon), (4, 0, 0, 'pictures', True))

# two wait at most: with both slots taken the picture is saved in the look, as without the switch (none is dropped)
order = []; gate = blocked()
for name in 'abc': core.picture_job(lambda name=name: order.append((name, threading.current_thread().name)))
check('a full queue: the third is saved in the look', order, [('c', 'MainThread')])
gate.set(); W0.flush()
check('...and the two waiting in their order', order, [('c', 'MainThread'), ('a', 'pictures'), ('b', 'pictures')])
check('...counted', (W0.n['queued'], W0.n['inline']), (7, 1))
n = len(said)
core.picture_job(lambda: 1 / 0); core.picture_job(lambda: order.append('after'))
check('a picture that fails: the writer goes on', (W0.flush(), order[-1], W0.n['failed'], W0.thread.is_alive()), (True, 'after', 1, True))
check('...and says so in one line', [s for s in said[n:] if 'picture not saved' in s], ["picture not saved: ZeroDivisionError('division by zero')"])
check('the time off the look is counted', W0.n['ms'] > 0 and W0.n['max'] > 0, True)

# pruning: the newest `kept` of a kind stay, off and on
class Clock:
    """core's clock, with a new second in every file name."""
    n = 0
    def __getattr__(self, name): return getattr(time, name)
    def strftime(self, fmt, *a): Clock.n += 1; return f'20261110_{Clock.n:06d}'
core.time = Clock(); series = [frame((160, 90)) for _ in range(5)]
D.use(None); Clock.n = 0
for im in series: core.keep_sample(im, 'runoff', gap=0, kept=2)
switches('proof-off-thread'); Clock.n = 0
for im in series: core.keep_sample(im, 'runon', gap=0, kept=2); W0.flush()
core.time = time
off_kept = [n for n in files('samples') if n.startswith('runoff')]; on_kept = [n for n in files('samples') if n.startswith('runon')]
check('off: the newest two of a run stay', off_kept, ['runoff_20261110_000004.jpg', 'runoff_20261110_000005.jpg'])
check('on: the same two', [n.replace('runon', 'runoff') for n in on_kept], off_kept)
check('on: with the same bytes', [content('samples', n) for n in on_kept] == [content('samples', n) for n in off_kept], True)

def fight_meter(folder, im, canvas=False):
    m = Meter.__new__(Meter)
    frames = os.path.join(TMP, 'frames_' + folder); os.makedirs(frames, exist_ok=True)
    m.fight = type('F', (), {'frames': frames, 'created': 1000.0, 'sampled': False, 'brk': type('K', (), {'phase': 'full'})(),
                             'log': type('L', (), {'name': os.path.join(TMP, folder + '.csv')})()})()
    m.last_frame = 0.0; m.canvas = im if canvas else None; m.grab = lambda: im
    m.args = type('A', (), {'cfg': {'capture': {'end_screens': True, 'debug_frames': True}}, 'replay': None})()
    m.after_fight = [folder, 2000.0, 0.0]
    return m
def kept_by(m, im, wide):
    """The three pictures of a fight the look thread writes: the bar's strip, the end screen, the bar sample."""
    bar = {'span': 1300 if wide else 400, 'left_bracket': 60, 'yc': 120}
    m.save_debug(im, bar, 1012.34); m.save_debug(im, bar, 1012.5)                # (the second is too soon: no picture)
    m.save_end_screens(im, 1012.34); m.save_end_screens(im, 1013.0)
    m.bar_sample(1012.34); m.bar_sample(1013.0)
def kept_of(folder):
    strip = os.listdir(os.path.join(TMP, 'frames_' + folder)); end = files('end_screens', folder)
    with open(os.path.join(TMP, 'frames_' + folder, strip[0]), 'rb') as f: s = f.read()
    return (strip, s, len(end), content('end_screens', folder, end[0]), content('bar_samples', folder + '.png'))
for wide in (False, True):
    tag = 'wide' if wide else 'std'; big = frame((1500, 400))
    D.use(None)
    pic = big.copy(); kept_by(fight_meter('off' + tag, pic), pic, wide)
    want = kept_of('off' + tag)
    check(f'off ({tag}): one strip, named by the fight\'s clock', want[0], ['0012.34.png'])
    switches('proof-off-thread')
    pic = big.copy(); m = fight_meter('on' + tag, pic, canvas=True)             # the frame is the reused canvas
    gate = blocked()
    kept_by(m, pic, wide)                                                        # (the third of them finds the queue full)
    pic.paste(Image.new('RGB', pic.size))                                        # ...which the next look paints over
    gate.set(); check(f'on ({tag}): written', W0.flush(), True)
    got = kept_of('on' + tag)
    check(f'on ({tag}): the same strip, end screen and bar sample', got == want, True)
    check(f'on ({tag}): the end screen is the frame, not the painted-over canvas', Image.open(os.path.join(TMP, 'logs', 'end_screens', 'on' + tag, files('end_screens', 'on' + tag)[0])).getextrema() != ((0, 0),) * 3, True)
check('all of them counted: two more saved in the look, none lost', (W0.n['queued'], W0.n['inline'], W0.n['failed']), (20, 3, 1))
check('proof-off-thread: guarded', ("if D.on('proof-off-thread') and WRITER.put(job): return" in source('core.py'),
                                     "if not D.on('proof-off-thread'): crop.save(path); return" in source('fights.py')), (True, True))
# a run's proof is never the writer's: what reads the file right after still finds it
check('proofs are not the writer\'s', [name for name in ('results.py', 'leaderboards.py', 'submit.py') if 'picture_job' in source(name)], [])

# ---------------------------------------------------------------- proof-writer-low
if os.name == 'nt':
    k32 = ctypes.WinDLL('kernel32'); k32.GetCurrentThread.restype = ctypes.c_void_p; k32.GetThreadPriority.argtypes = [ctypes.c_void_p]
    seen = []
    def note(): seen.append(k32.GetThreadPriority(k32.GetCurrentThread()))
    W0.put(note); W0.flush()
    switches('proof-off-thread', 'proof-writer-low'); W0.put(note); W0.flush()
    switches('proof-off-thread'); W0.put(note); W0.flush()
    check('the writer: normal priority, lowest with its own switch, normal again', seen, [0, -2, 0])
    check('...and this thread\'s is never touched', k32.GetThreadPriority(k32.GetCurrentThread()), 0)
D.use(None)

# ---------------------------------------------------------------- quiet-top-only
def after_match(read_due=False, sent=990.0, **over):
    """Out of the run, a match's result sent `1000 - sent` seconds ago: what full_frame_due asks the capture for at 1000."""
    m = Meter.__new__(Meter)
    m.end_sig = None; m.after_fight = None; m.holosim_space = None; m.bossrush_space = None; m.auto_until = 0.0
    m.args = type('A', (), {'cfg': {'capture': {'end_screens': True, 'nobar_frames': False}}, 'replay': None})()
    m.sanctum_in_run = lambda now: False; m.last_fight = None
    m.screen_bg = type('B', (), {'busy': False})(); m.names_waiting = lambda: False
    m.asked = []; m.screen_read_due = lambda now, quick=False: (m.asked.append(quick), read_due)[1]
    m.full_until = 0.0; m.egg_sent_at = sent
    for name, v in over.items(): setattr(m, name, v)
    return m
def asks(**k):
    m = after_match(**k); return m.full_frame_due(1000.0), getattr(m, 'dev_n', {})
ANYWHERE = (dict(sent=900.0), dict(sent=955.0), dict(sent=0.0))                  # not after a match: 45 s and more
def everything():
    """full_frame_due's answer in every state tried here, the 45 s after a match and outside them."""
    out = []
    for base in (dict(), *ANYWHERE):
        for due in (False, True):
            for more in (dict(), dict(full_until=2000.0), dict(end_sig=b'x'), dict(egg_hold=[1, 0, 'x']), dict(sim_tour=[1]), dict(sim_ctl=[1]),
                         dict(screen_bg=None), dict(screen_bg=type('B', (), {'busy': True})()), dict(names_waiting=lambda: True),
                         dict(after_fight=['f', 2000.0, 990.0]), dict(after_fight=['f', 2000.0, 999.0]),
                         dict(last_fight=object(), fight_ended_at=985.0), dict(last_fight=object(), fight_ended_at=970.0),
                         dict(sanctum_in_run=lambda now: True)):
                out.append(asks(read_due=due, **base, **more)[0])
    return out
D.use(None)
today = everything()
check('off, as today: whole frames on every look for 45 s after a match', (asks(), asks(read_due=True)), ((True, {}), (True, {})))
m = after_match(); m.full_frame_due(1000.0)
check('off: the read isn\'t even asked about', m.asked, [])
switches('quiet-top-only')
check('on: only the top part between screen reads', asks(), (False, {'quiet_top': 1}))
check('on: a whole frame (never just the boxes) for a screen read', asks(read_due=True), (True, {'quiet_whole': 1}))
check('on: ...also once a screen is up', asks(read_due=True, full_until=2000.0)[0], True)
check('on: no read while the last one is still being read', asks(read_due=True, screen_bg=type('B', (), {'busy': True})())[0], False)
check('on: nor while names are read', asks(read_due=True, names_waiting=lambda: True)[0], False)
check('on: an input hold still gets whole frames', asks(egg_hold=[1, 0, 'x'])[0], True)
check('on: an end screen being watched too', asks(end_sig=b'x')[0], True)
check('on: 20 s after an Interlink fight too (the leader\'s line)', asks(last_fight=object(), fight_ended_at=985.0)[0], True)
check('on: ...not after those', asks(last_fight=object(), fight_ended_at=970.0)[0], False)
check('on: an end-screen picture that is due too', asks(after_fight=['f', 2000.0, 990.0])[0], True)
m = after_match(); m.full_frame_due(1000.0)
check('on: the read is asked about as every other screen\'s', m.asked, [False])
m = after_match(auto_until=time.time() + 100); m.full_frame_due(1000.0)
check('on: at once while an automatic click waits for its screen', m.asked, [True])
got = everything()
inside = len(today) // 4                                                         # the states of the 45 s come first
check('on: outside the 45 s nothing changes', got[inside:], today[inside:])
check('on: inside them, whole frames never come where there were none', [g for g, t in zip(got[:inside], today[:inside]) if g and not t], [])
check('on: no boxes-only frame in the 45 s (the rank screen is kept from a whole one)', 'screen' in got[:inside], False)
check('off: every look of the 45 s was a whole frame', set(today[:inside]), {True})
check('outside them: boxes for a read, whole with a screen up, the top part otherwise',
      (asks(sent=900.0, read_due=True), asks(sent=900.0, read_due=True, full_until=2000.0), asks(sent=900.0)), (('screen', {}), (True, {}), (False, {})))
check('quiet-top-only: guarded', "if egg_after and not D.on('quiet-top-only'): return True" in source('live.py'), True)
D.use(None)

# ---------------------------------------------------------------- busy-half-pace
CHANGED, SAME = bytes([40]) * 1296, bytes(1296)
def run_meter(**over):
    """In an Egg Heist run or an Interlink room, nothing going on; the last screen read at 1000."""
    m = Meter.__new__(Meter)
    m.fight = None; m.bar = None; m.args = type('A', (), {'cfg': {}, 'replay': None})(); m.idle = lambda: False
    m.end_sig = None; m.auto_until = 0.0; m.pending_run = None; m.awaiting = None; m.send_waiting = None
    m.hold = type('H', (), {'active': False})(); m.holosim_space = None; m.bossrush_space = None
    m.names_waiting = lambda: False; m.after_fight = None; m.clicks = 1
    m.screen_read_sig = SAME; m.top_sig = CHANGED; m.last_results = 1000.0     # the screen changed since the last read
    for name, v in over.items(): setattr(m, name, v)
    return m
def due(at, quick=False, **over): return run_meter(**over).screen_read_due(1000.0 + at, quick)
CANT_WAIT = (('a screen just spotted or a fade just seen', dict(calm_until=2000.0)), ('a screen is up', dict(full_until=2000.0)),
             ('an end screen is watched', dict(end_sig=b'x')), ('an automatic click', dict(auto_until=time.time() + 100)),
             ('a run waits for its screen', dict(pending_run={'a': 1})), ('a result is awaited', dict(awaiting=1)),
             ('a result waits to be sent', dict(send_waiting=1)), ('inputs are held', dict(hold=type('H', (), {'active': True})())),
             ('Holo-Sim', dict(holosim_space=1)), ('names are read', dict(names_waiting=lambda: True)),
             ('Egg Heist: inputs held', dict(egg_hold=[1, 0, 'x'])), ('Egg Heist: the end screen is read', dict(egg_read={'a': 1})),
             ('Egg Heist: the rank screen is read', dict(egg_points={'a': 1})), ('Egg Heist: a result is kept', dict(egg_pending={'a': 1})),
             ('Egg Heist: a click is due', dict(egg_click=1.0)), ('a tour', dict(sim_tour=[1])), ('Holo-Sim\'s controller', dict(sim_ctl=[1])),
             ('45 s after a fight', dict(after_fight=['f', 2000.0, 0.0])), ('90 s after a sent match', dict(egg_sent_at=911.0)),
             ('90 s after a match\'s end screen', dict(egg_end_seen=911.0)), ('a fight', dict(fight=object())), ('a boss bar', dict(bar={'yc': 1})))
def pace():
    """When the next screen read is due, in every state tried here."""
    out = [due(at, **over) for _, over in (('calm', {}),) + CANT_WAIT for at in (0.05, 0.1, 0.2, 0.3, 0.49, 0.5, 1.0, 1.5)]
    out += [due(at, top_sig=SAME) for at in (0.2, 0.5, 1.49, 1.5)]                        # a still screen
    out += [due(at, True) for at in (0.1, 0.25)] + [due(at, idle=lambda: True) for at in (0.2, 0.5, 0.99, 1.0)]
    return out
D.use(None)
today = pace()
check('off, as today: a changed screen is read 0.1 s after the last read', (due(0.05), due(0.1), due(0.2)), (False, True, True))
check('off: in every state tried here', {tuple(today[i:i + 8]) for i in range(0, 8 * (1 + len(CANT_WAIT)), 8)}, {(False,) + (True,) * 7})
switches('busy-half-pace')
check('on, calm: 2 reads a second', (due(0.1), due(0.2), due(0.49), due(0.5)), (False, False, False, True))
check('on: a still screen every 1.5 s, as today', (due(1.49, top_sig=SAME), due(1.5, top_sig=SAME)), (False, True))
check('on: a quick read (a fight\'s bar hidden) as today', (due(0.1, True), due(0.25, True)), (False, True))
check('on: idle in the world as today (once a second)', (due(0.99, idle=lambda: True), due(1.0, idle=lambda: True)), (False, True))
check('on: calm it is', run_meter().busy_calm(1000.2), True)
for name, over in CANT_WAIT:
    m = run_meter(**over)
    check(f'on: {name}: not calm, today\'s pace', (m.busy_calm(1000.2), m.screen_read_due(1000.1), m.screen_read_due(1000.2)), (False, True, True))
check('on: idle is never calm (its own pace)', run_meter(idle=lambda: True).busy_calm(1000.2), False)
check('on: a replay is never calm', run_meter(args=type('A', (), {'cfg': {}, 'replay': 'x'})()).busy_calm(1000.2), False)
check('on: 90 s after a match it is calm again', (run_meter(egg_sent_at=910.3).busy_calm(1000.2), run_meter(egg_sent_at=910.1).busy_calm(1000.2)), (False, True))
check('on: 45 s after a fight too', run_meter(after_fight=['f', 1000.1, 0.0]).busy_calm(1000.2), True)
check('on: a screen spotted is past', (run_meter(calm_until=1000.3).busy_calm(1000.2), run_meter(calm_until=1000.2).busy_calm(1000.2)), (False, True))
got = pace(); calm = 8
check('on: only the calm state\'s reads move', got[calm:], today[calm:])
check('on: and only later, never past 0.5 s', (today[:calm], got[:calm]), ([False, True, True, True, True, True, True, True], [False, False, False, False, False, True, True, True]))
# what puts today's pace back: the top part jumping from one look to the next, and a fade to black
def lum(v, bright=0): return bytes([v]) * (1296 - bright) + bytes([255]) * bright
m = run_meter(); m.calm_watch(lum(100), 1000.0)
check('the first look: today\'s pace for 1.5 s', m.calm_until, 1001.5)
m.calm_watch(lum(100), 1002.0); m.calm_watch(lum(124), 1003.0)
check('a still screen, a gentle change: nothing', m.calm_until, 1001.5)
m.calm_watch(lum(149), 1004.0)
check('a jump (a screen opening): 1.5 s', m.calm_until, 1005.5)
m.calm_watch(bytes([100]) * 100, 1005.0)
check('another frame size: 1.5 s', m.calm_until, 1006.5)
m = run_meter(calm_sig=lum(5)); m.calm_watch(lum(5), 1000.0)
check('black (a fade): 10 s, on every look of it', m.calm_until, 1010.0)
m.calm_watch(lum(5, 60), 1001.0)
check('black with a few bright points is black', m.calm_until, 1011.0)
m.calm_watch(lum(5, 100), 1002.0)
check('...with more of them it isn\'t (and no jump either)', m.calm_until, 1011.0)
m.calm_watch(lum(200), 1003.0)
check('a jump never shortens it', m.calm_until, 1011.0)
m = run_meter(calm_sig=lum(12)); m.calm_watch(lum(14), 1000.0)
check('dark, not black: nothing', getattr(m, 'calm_until', 0), 0)
m = run_meter()
check('the looks: 10 a second', (m.calm_pace(0.05), m.calm_pace(0.25)), (0.1, 0.25))
m.bossrush_space = 'room'
check('...except in Interlink rooms (a boss bar starts a timed fight there)', m.calm_pace(0.05), 0.05)
check('...counted for the log', m.dev_n, {'calm': 3})
src = source('live.py')
check('busy-half-pace: guarded (the reads)', "elif not quick and gap < self.CALM_READ_S and D.on('busy-half-pace') and self.busy_calm(now): gap = self.CALM_READ_S" in src, True)
check('busy-half-pace: guarded (the looks)', "elif D.on('busy-half-pace') and self.busy_calm(time.time()): interval = self.calm_pace(interval)" in src, True)
check('busy-half-pace: guarded (the watch)', "if not self.fight and self.bar is None and D.on('busy-half-pace'): self.calm_watch(self.top_sig, now)" in src, True)
D.use(None)

# ---------------------------------------------------------------- area-keep-short
TOP, WHOLE = (0, 0, 65535, 27525), (0, 0, 65535, 65535)
def capture():
    c = wc.SharedCapture.__new__(wc.SharedCapture)
    c.slot = 0; c.areas = []; c.box = None; c.request = 0; c.boxes = {}
    c.memory = ctypes.create_string_buffer(8192); c.at = ctypes.addressof(c.memory)
    return c
def asked_for(c, at, area):
    c._ask_area(c.at, at, area)
    want = tuple(ctypes.c_uint16.from_address(c.at + c.PICTURE_BASE + 2 * k).value for k in range(4))
    return want if ctypes.c_uint32.from_address(c.at + c.PICTURE_BASE + 8).value == c.request else 'the request id is not the last one'
def whole_then_top(times):
    c = capture(); out = [asked_for(c, 100.0, WHOLE)] + [asked_for(c, 100.0 + t, TOP) for t in times]
    return ['whole' if b == WHOLE else 'top' if b == TOP else b for b in out], c
TIMES = (0.05, 0.25, 0.35, 1.0, 1.9, 2.1)
D.use(None)
got, c = whole_then_top(TIMES)
check('off, as today: a whole frame stays asked for 2 s', (got, c.asked, c.grown), (['whole'] * 6 + ['top'], 2, 0))
switches('area-keep-short')
got, c = whole_then_top(TIMES)
check('on: for 0.3 s', (got, c.asked, c.grown), (['whole'] * 3 + ['top'] * 4, 2, 0))
check('on: the next whole frame is a new request, for more rows', (asked_for(c, 103.0, WHOLE), c.asked, c.grown), (WHOLE, 3, 1))
c = capture()
got = [asked_for(c, 100.0 + i * 0.25, WHOLE if i % 2 == 0 else TOP) for i in range(9)]
check('on: reads a quarter second apart keep the whole frame', (set(got), c.asked), ({WHOLE}, 1))
check('area-keep-short: guarded', "keep = self.AREA_SHORT if D.on('area-keep-short') else self.AREA_KEEP" in source('wincapture.py'), True)
check('the times', (wc.SharedCapture.AREA_KEEP, wc.SharedCapture.AREA_SHORT), (2.0, 0.3))
D.use(None)

# ---------------------------------------------------------------- helpers-below-normal
BELOW, NORMAL = 0x4000, 0x20
started = []
class Helper:
    def __init__(self, text): self.text = text; self.killed = False
    @property
    def stdout(self): return type('S', (), {'readline': staticmethod(lambda: 'READY\ten-US\n' if self.text else b'READY\ten-US\n')})()
    def kill(self): self.killed = True
class Sub:
    """reader's subprocess module, starting nothing."""
    def __getattr__(self, name): return getattr(subprocess, name)
    def Popen(self, cmd, **k): started.append(k.get('creationflags', 0)); return Helper(k.get('text'))
def launch(native):
    o = R.Ocr.__new__(R.Ocr); o.native = native
    R.subprocess = Sub()
    try: return o.launch()[1], started.pop()
    finally: R.subprocess = subprocess
if os.name == 'nt':
    check('off, as today: a helper starts at normal priority, no window', [launch(n) for n in (True, False)], [('en-US', subprocess.CREATE_NO_WINDOW)] * 2)
    switches('helpers-below-normal')
    check('on: below normal, still no window', [launch(n) for n in (True, False)], [('en-US', subprocess.CREATE_NO_WINDOW | BELOW)] * 2)
    D.use(None)
    # Windows takes it: a process started that way, and one already running moved there and back
    k32.GetPriorityClass.argtypes = [ctypes.c_void_p]; k32.GetPriorityClass.restype = ctypes.c_uint32
    def stand_in(flags=0): return subprocess.Popen([sys.executable, '-c', 'import sys; sys.stdin.read()'], stdin=subprocess.PIPE, creationflags=subprocess.CREATE_NO_WINDOW | flags)
    def priority(p): return k32.GetPriorityClass(int(p._handle))
    low = stand_in(subprocess.BELOW_NORMAL_PRIORITY_CLASS); o = R.Ocr.__new__(R.Ocr); o.p = stand_in()
    try:
        check('a process started with the flag is below normal', priority(low), BELOW)
        check('a running helper: to below normal', (o.set_priority(True), priority(o.p)), (True, BELOW))
        check('...and back', (o.set_priority(False), priority(o.p)), (True, NORMAL))
    finally:
        for p in (low, o.p): p.stdin.close(); p.wait(10)
o = R.Ocr.__new__(R.Ocr); o.p = None
check('no helper: no error', (o.set_priority(True), o.set_priority(False)), (False, False))

# the helpers already running follow the switch, at the meter's once-a-minute line; that line says what the switches did
class Reader:
    def __init__(self): self.set = []
    def set_priority(self, low): self.set.append(low); return True
def line_meter():
    m = Meter.__new__(Meter)
    m.ocr = Reader(); m.ocr_bg = type('B', (), {'ocr': Reader()})(); m.screen_bg = type('B', (), {'ocr': None})(); m.hp_ocr = Reader()
    return m, (m.ocr, m.ocr_bg.ocr, m.hp_ocr)
def lines(n): return [s for s in said[n:] if not s.startswith('dev switches')]      # (the flag module's own line)
m, helpers = line_meter(); m.dev_line(940.0); n = len(said)                      # (its first line: this test's work so far)
m.dev_line(1000.0); m.dev_line(1060.0)
check('off: the helpers are left alone, and nothing is said', ([h.set for h in helpers], lines(n)), ([[]] * 3, []))
switches('helpers-below-normal'); m.dev_line(1120.0); m.dev_line(1180.0)
check('on: each running helper once', [h.set for h in helpers], [[True]] * 3)
check('...said', lines(n), ['dev helpers-below-normal: 3 of 3 OCR helpers now at Below Normal priority'])
D.use(None); m.dev_line(1240.0); m.dev_line(1300.0)
check('off again: back to normal, once', ([h.set for h in helpers], said[-1]), ([[True, False]] * 3, 'dev helpers-below-normal: 3 of 3 OCR helpers now at Normal priority'))
check('helpers-below-normal: guarded', "if D.on('helpers-below-normal'): flags |= getattr(subprocess, 'BELOW_NORMAL_PRIORITY_CLASS', 0)" in source('reader.py'), True)

m, _ = line_meter(); m.dev_line(940.0); n = len(said)
check('the line: the first one says what was done so far', said[-1].startswith('dev: ') and ' fill fast, ' in said[-1], True)
m.dev_line(1000.0)
check('the line: nothing done since, nothing said', lines(n), [])
R.DEV['fill_fast'] += 7; X.SAME['reads'] += 5; X.SAME['same'] += 4; m.dev_count('calm', 3); m.dev_count('quiet_top', 2); m.dev_count('search_rest')
wc.shared.asked += 9; W0.n['inline'] += 1
m.dev_line(1060.0)
check('the line: what was done since the last one', lines(n), ['dev: 7 fill fast, 5 xN read, 4 xN same as last, 1 pictures saved in the look, 3 calm looks, 2 top parts after a match, 1 bar searches skipped'])
m.dev_line(1120.0)
check('the line: a quiet minute says nothing', len(lines(n)), 1)
switches('area-keep-short', 'proof-off-thread'); wc.shared.asked += 4; wc.shared.grown += 2
W0.put(lambda: None); W0.flush(); m.dev_line(1180.0)
check('the line: area requests with their switch, pictures with their time', re.sub(r'\d+ ms', 'N ms', re.sub(r'longest \d+', 'longest N', said[-1])),
      'dev: 1 pictures queued, 4 area requests, 2 for more rows, pictures took N ms off the look (longest N)')
check('the line: counts and words only', [s for s in lines(n) if not re.fullmatch(r'dev: [0-9a-zA-Z ,()]+', s)], [])
D.use(None); wc.shared.asked = wc.shared.grown = 0
m.ocr = None; del m.ocr_bg                                                       # (a meter half built: the line is never an error)
switches('helpers-below-normal'); m.dev_line(1240.0); D.use(None)
check('the line: an error is only a line', said[-1].startswith('dev: line not written'), True)

# ---------------------------------------------------------------- tabbed-out-rest
def tabbed(front=False, **over):
    m = Meter.__new__(Meter); m.fight = None; m.bar = None; m.hwnd = 1; m.args = type('A', (), {'cfg': {}, 'replay': None})()
    for name, v in over.items(): setattr(m, name, v)
    return m
real_front = live.wc.is_foreground
LOOKS = [1000.0 + i * 0.25 for i in range(9)]                                    # tabbed out, the meter looks 4 times a second
try:
    live.wc.is_foreground = lambda hwnd: False
    D.use(None); m = tabbed()
    check('off, as today: the whole frame is searched on every look', ([m.search_rests(t) for t in LOOKS], getattr(m, 'dev_n', {})), ([False] * 9, {}))
    switches('tabbed-out-rest'); m = tabbed()
    check('on, tabbed out: every other look (twice a second)', ([m.search_rests(t) for t in LOOKS], m.dev_n), ([False, True] * 4 + [False], {'search_rest': 4}))
    m = tabbed(); gaps = [t for t in (1000.0 + i * 0.05 for i in range(60)) if not m.search_rests(t)]
    check('on: never more than half a second without a search', max(b - a for a, b in zip(gaps, gaps[1:])) < 0.56, True)
    for name, over in (('a fight', dict(fight=object())), ('a boss bar', dict(bar={'yc': 1})), ('no game window', dict(hwnd=None)),
                       ('a replay', dict(args=type('A', (), {'cfg': {}, 'replay': 'x'})()))):
        m = tabbed(**over)
        check(f'on, {name}: every look', [m.search_rests(t) for t in LOOKS], [False] * 9)
    live.wc.is_foreground = lambda hwnd: True
    m = tabbed()
    check('on, the game in front: every look', ([m.search_rests(t) for t in LOOKS], getattr(m, 'dev_n', {})), ([False] * 9, {}))
finally: live.wc.is_foreground = real_front
check('tabbed-out-rest: the known spots are still checked on every look',
      'found = R.bar_at(im, self.bar_spots) or (None if self.search_rests(now) else R.find_bar(im))' in source('live.py'), True)
check('tabbed-out-rest: guarded', "if not D.on('tabbed-out-rest') or wc.is_foreground(self.hwnd): return False" in source('live.py'), True)

D.use(None)
if fails:
    print('\n'.join(fails)); sys.exit(1)
print('dev look loop: ok')
