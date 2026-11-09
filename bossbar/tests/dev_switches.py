"""Beta Menu > Dev: the flag module, and each switch on and off.

    python tests/dev_switches.py

- devflags: off without the app's word (a release build), reads what the app writes, looks at most once a second, never
  raises on a missing or broken file;
- every devflags.on('...') in the meter is on the list in app/ui/dev-options.json, and every entry has what the tab shows;
- fight-bar-slide-in: off keeps today's clock, on starts it at the real first hit (fills from real fights, numbers only);
- egg-gain-limit: off keeps today's gain, on takes one above the tier's limit as a misread; every gain in real logs stays;
- kill-sound-log: off says nothing, on says why, and can't break the end of a fight.
Readers and sending are stubbed. Exit code 1 on any failure.
"""
import glob, json, logging, os, re, sys, tempfile
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
TMP = tempfile.mkdtemp(prefix='alogs_dev_')
os.environ['ALOGS_DATA'] = TMP
os.environ.pop('ALOGS_DEV_ON', None)
from PIL import Image
import devflags as D
import eggheist as E
import live, reader as R
import fights, results
from live import Meter
logging.getLogger('meter').addHandler(logging.NullHandler()); logging.getLogger('meter').propagate = False

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

FLAGS = os.path.join(TMP, 'dev-on.json')
def switches(*on):
    """As the app writes it: whole, then the meter looks."""
    with open(FLAGS + '.tmp', 'w', encoding='utf-8') as f: json.dump({'on': list(on)}, f)
    os.replace(FLAGS + '.tmp', FLAGS)
    D.use(FLAGS)

# ---------------------------------------------------------------- the flag module
D.use(None)
looked = []; real_stat = os.stat
os.stat = lambda *a, **k: (looked.append(a), real_stat(*a, **k))[1]
check('release build: off', D.on('fight-bar-slide-in'), False)
os.stat = real_stat
check('release build: no file is looked at', looked, [])

switches('a', 'b')
check('on: what the app says', (D.on('a'), D.on('b'), D.on('c')), (True, True, False))
D.use(os.path.join(TMP, 'none.json'))
check('no file: off', D.on('a'), False)
with open(FLAGS, 'w', encoding='utf-8') as f: f.write('{not json')
D.use(FLAGS)
check('a broken file: off', D.on('a'), False)
for text in ('[1]', '{"on": 3}', '{"on": [1, null, "a"]}', '{}'):
    with open(FLAGS, 'w', encoding='utf-8') as f: f.write(text)
    D.use(FLAGS)
    check(f'odd contents {text}: no error', D.on('b'), False)

switches('a')
looked = []; os.stat = lambda *a, **k: (looked.append(a), real_stat(*a, **k))[1]
D._next = 0.0
for _ in range(1000): D.on('a')
os.stat = real_stat
check('1000 calls look at the file once', len(looked), 1)
with open(FLAGS, 'w', encoding='utf-8') as f: json.dump({'on': ['z', 'y']}, f)
D._next = 0.0
check('a change is seen at the next look', (D.on('a'), D.on('z')), (False, True))
os.remove(FLAGS); D._next = 0.0
check('the file gone again: off', D.on('z'), False)
D._next = 0.0; D.use(None)

# ---------------------------------------------------------------- the list
OPTIONS = os.path.join(HERE, '..', 'app', 'ui', 'dev-options.json')
with open(OPTIONS, encoding='utf-8') as f: options = json.load(f)
ids = {o['id'] for o in options}
check('ids are unique', len(ids), len(options))
for o in options:
    for field in ('id', 'plugin', 'section', 'area', 'title', 'what', 'gain', 'risk', 'tested'):
        if not (isinstance(o.get(field), str) and o[field]): fails.append(f'{o.get("id")}: {field} missing')
    check(f"{o['id']} plug-in", o['plugin'] in ('core', 'dps', 'leaderboards'), True)
    check(f"{o['id']} section", o['section'] in ('No known risk', 'Low risk', 'High risk'), True)
    check(f"{o['id']} area", o['area'] in ('CPU', 'Smoothness', 'Accuracy', 'Memory', 'Speed'), True)
    check(f"{o['id']} revision", isinstance(o.get('revision'), int), True)
    for field in ('title', 'what', 'gain', 'risk', 'tested'):         # player text: plain words
        if re.search(r'\w+\.(py|rs|js|json|log|exe|csv)\b', o[field]): fails.append(f"{o['id']}: a file name in {field}")
guarded = set()
for path in glob.glob(os.path.join(HERE, '*.py')):
    with open(path, encoding='utf-8') as f: guarded |= set(re.findall(r"\bD\.on\('([^']+)'\)", f.read()))
app_src = os.path.join(HERE, '..', 'app', 'src-tauri', 'src')
for path in glob.glob(os.path.join(app_src, '*.rs')):
    if os.path.basename(path) == 'dev.rs': continue
    with open(path, encoding='utf-8') as f: guarded |= set(re.findall(r'dev::on\("([^"]+)"\)', f.read()))
check('every switch in the code is on the list', sorted(guarded - ids), [])
check('every switch on the list is in the code', sorted(ids - guarded), [])

# ---------------------------------------------------------------- fight-bar-slide-in
RED = (250, 110, 110)
row = next(r for r in R.load_catalog() if r['puppetId'] == '2104030001')
GRADES = R.load_grades()
def fight(fills, gap=1.0):
    """A world fight fed these readings, a gap apart (None: a frame that isn't the bar's red). (the fight, its first hit's
    time, or None, on a clock starting at 0.)"""
    f = live.Fight('test', 65, row, 329_764, False)
    try:
        for i, fill in enumerate(fills):
            f.update(fill if fill is not None else 1.0, RED if fill is not None else (120, 120, 120), RED, None, None, GRADES, 180, at=f.created + i * gap)
    finally: f.close()
    return f, (f.t0 - f.created if f.hit else None)

# 8 Nov: the bar read 8.3-9.0% as it slid in, then 100.0, 99.4, 95.5 ... down to 5.4 about 15 s later (one reading a second).
SLID_IN = [0.09, 0.09, 1.00, 0.994, 0.955, 0.949, 0.896, 0.861, 0.804, 0.749, 0.710, 0.694, 0.543, 0.492, 0.139, 0.054]
D.use(None)
off, off_hit = fight(SLID_IN)
check('off, as today: the low reading sticks, the first hit is only at 5.4%', (off.hit, off_hit > 14), (True, True))
check('off, as today: 5.4% left', round(off.seg_fill, 3), 0.054)
switches('fight-bar-slide-in')
f, hit = fight(SLID_IN)
check('on: the clock starts at the real first hit', 2 < hit < 5, True)
check('on: and the same HP at the end', round(f.seg_fill, 3), 0.054)
check('on: the damage is the HP gone', f.damage, round(329_764 * (1 - 0.054)))
# the other four fights in the logs with this: (the low reading, then what followed)
for low, then in ((0.089, [0.997, 0.997, 0.996, 0.989, 0.974]), (0.034, [0.961, 0.952, 0.952, 0.948, 0.90]), (0.084, [0.435, 0.435, 0.435, 0.42, 0.41])):
    f, hit = fight([low, low] + then)
    check(f'on: {low} then {then[0]}: the low one goes', (f.hit, round(f.seg_fill, 3), hit is not None and hit < 7), (True, then[-1], True))
    D.use(None)
    f, hit = fight([low, low] + then)
    check(f'off: {low} then {then[0]}: never hit', (f.hit, round(f.seg_fill, 3)), (False, low))
    switches('fight-bar-slide-in')
# what it must leave alone, on as off
for name, fills in (('a normal fight', [1.0, 1.0, 0.99, 0.9, 0.8, 0.7]),
                    ('joined late, and it stays low', [0.3, 0.3, 0.3, 0.29, 0.28]),
                    ('one fuller reading among low ones', [0.3, 0.3, 0.9, 0.3, 0.29]),
                    ('a low frame in a fight already going', [1.0, 0.9, 0.2, 0.9, 0.85])):
    a, a_hit = fight(fills)
    D.use(None)
    b, b_hit = fight(fills)
    switches('fight-bar-slide-in')
    check(f'{name}: the same on as off', (a.seg_fill, a.hit, a.damage, None if a_hit is None else round(a_hit, 3)), (b.seg_fill, b.hit, b.damage, None if b_hit is None else round(b_hit, 3)))
# after a hit, HP only drops, as ever
f, _ = fight([1.0, 0.9, 0.5, 0.5, 0.99, 0.99, 0.99])
check('after the first hit a fuller reading changes nothing', round(f.seg_fill, 3), 0.5)
# Interlink is left alone
g = live.Fight('test', 65, row, 329_764, True)
try:
    for i, fill in enumerate((0.09, 0.09, 1.0, 1.0, 1.0)): g.update(fill, RED, RED, None, None, GRADES, 180, at=g.created + i)
    check('Interlink is left alone', g.rise_at, None)
finally: g.close()
D.use(None)

# ---------------------------------------------------------------- egg-gain-limit
check('the limits', E.POINT_LIMIT, {1: 90, 2: 180, 3: 270, 4: 360, 5: 360, 6: 360, 7: 360})
# Every gain read in real logs, by tier: none is above its limit (numbers only)
for tier, gains in ((3, [53]), (4, [117, 117, 117, 180, 180, 180, 180, 180, 204, 204, 204, 204, 204, 204])):
    for gain in gains: check(f'tier {tier}, +{gain} from a real log stays', E.gain_possible(gain, tier), True)
check('at the limit stays', E.gain_possible(90, 1), True)
check('above it goes', E.gain_possible(91, 1), False)
check('a loss stays', E.gain_possible(-500, 1), True)
check('an unknown tier stays', E.gain_possible(9999, None), True)

U, H = 3840, 2160
IM = Image.new('RGB', (U, H))
R.prep_pill = lambda *a, **k: 'a picture of the pill'    # the made-up picture has no pill; the OCR stub gives its text
class Rank(Meter):
    pass
def rank_screen(gain_text, points='1000/1350', bar_before=None):
    """One read of the rank screen: what it sends. Title 'Rising Egg Scout II' is tier 2 (limit 180)."""
    m = Rank.__new__(Rank)
    class Ocr:
        def read(self, crops, **k):
            if 'high' in crops: return {'high': points, 'low': ''}
            return {'title': 'Rising Egg Scout II', 'title2': '', **{name: gain_text for name in crops if name.startswith('gain')}}
    sent = []
    class Sub:
        def enqueue(self, payload, shot, **k): sent.append(payload)
    m.ocr = Ocr(); m.submitter = Sub(); m.uid = '1'; m.egg_season = 'S1'; m.last_scores = {}; m.out = lambda *a: None; m.egg_points = None
    m.egg_gain_seen = 0; m.egg_rank_seen = lambda *a: None
    kept = {'bar': [bar_before, 1350]} if bar_before is not None else {}
    profile = {}
    def egg_profile(season, **values):
        profile.update({k: v for k, v in values.items() if v is not None}); return {**kept, **profile}
    m.egg_profile = egg_profile
    m.egg_rank_gain(IM, 100.0, U, 0, H, once=True)
    return profile.get('lastGain'), len(sent)
D.use(None)
check('off: a big gain goes, as today', rank_screen('+1800'), (1800, 1))
check('off: within the limit goes', rank_screen('+18'), (18, 1))
check('off: the total\'s change goes, as today', rank_screen('', bar_before=100), (900, 1))
switches('egg-gain-limit')
check('on: within the limit goes', rank_screen('+18'), (18, 1))
check('on: at the limit goes', rank_screen('+180'), (180, 1))
check('on: above it is a misread, and the total still goes', rank_screen('+1800'), (None, 1))
check('on: a loss goes', rank_screen('-30'), (-30, 1))
check('on: the total\'s change above it: matches went unread, no gain', rank_screen('', bar_before=100), (None, 1))
check('on: the total\'s change within it goes', rank_screen('', bar_before=900), (100, 1))
check('on: a misread falls back on the total\'s change', rank_screen('+1800', bar_before=900), (100, 1))
D.use(None)

# ---------------------------------------------------------------- kill-sound-log
said = []
class Hear(logging.Handler):
    def emit(self, record): said.append(record.getMessage())
logging.getLogger('meter').addHandler(Hear()); logging.getLogger('meter').setLevel(logging.INFO)
class Audio:
    state = (1, 1000.0, 0.41, 996.4)
    def cue(self): return None
    def listening(self): return self.state
real_audio = fights.wc.audio
fights.wc.audio = Audio()
m = Meter.__new__(Meter)
m.catch_seen = 1000.0; m.args = type('A', (), {'cfg': {'audio_cues': {'boss_kill': True}}, 'replay': None})()
m.kill_sound_note(None)
line = said[-1] if said else ''
check('it says it wasn\'t used', 'boss kill sound not used' in line, True)
check('it says the app was listening', 'listening' in line, True)
check('it gives the best match and when', 'best match 0.41' in line and '3.6s before' in line, True)
check('it gives the last audio', 'last heard 0.0s' in line, True)
n = len(said)
fights.wc.audio.listening = lambda: None
m.kill_sound_note(None)
check('without the app it still says', "can't be read" in said[n], True)
def boom(): raise OSError('gone')
fights.wc.audio.listening = boom
m.kill_sound_note(1.0)
check('an error is only a line', 'no note' in said[-1], True)
fights.wc.audio = real_audio
src = open(os.path.join(HERE, 'fights.py'), encoding='utf-8').read()
check('it runs only with the switch on', "if D.on('kill-sound-log'): self.kill_sound_note(heard)" in src, True)

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('dev switches: ok')
