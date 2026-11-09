"""Egg Heist's end screen: the game's digits, two reads alike, one record per match.

    python tests/egg_result_reads.py

Meter.egg_result (8 Nov: Loot 40939 went as 4, Total Defeats blank, and one match went twice, once as "Adventure"):
- nothing goes on one read, and a number that only one read saw isn't sent;
- a title that isn't clearly won or lost isn't an outcome, and doesn't make a second record;
- later reads of the same screen, even with another number, make no second record;
- a screen that leaves early sends what agreed, with the rest blank;
- a screen that never agrees goes after EGG_MAX_READS, with the rest blank;
- the match goes at once, and the rank screen's picture updates it; inputs are held while they're read;
- without the game's font the OCR read is used, still two alike, and only when all of it is a number.
Meter.party_screen: a card that reads blank doesn't stop the read (INTERLINK_NPCS reaches live.py).
Meter.party_layout, egg_send: Egg Heist's Party screen by its own layout; the party goes with its match, once.
Made-up pictures and numbers; readers and sending are stubbed. Exit code 1 on any failure.
"""
import os, sys, tempfile
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_egg_')
from PIL import Image
import live, results
from live import Meter
import logging; logging.disable(logging.CRITICAL)

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

U, H = 3840, 2160
IM = Image.new('RGB', (U, H))
class Reader:
    def __init__(self): self.loot = 1234; self.defeats = 1
    def number(self, im, box, **k): return self.loot if box[1] < H * 0.36 else self.defeats
class Ocr:
    time = "'02:12"; extra = {}
    def read(self, crops, **k):
        return {'difficulty': 'Normal', 'mode': 'Operation: Egg Heist Team Mode', 'time': self.time, 'goal': '', 'chests': '', **self.extra}
class Sub:
    def __init__(self): self.sent = []
    def enqueue(self, payload, shot, battle=None, party=None): self.sent.append(payload); self.files = getattr(self, 'files', []) + [(shot, battle, party)]
def meter(reader=True):
    m = Meter.__new__(Meter); m.ocr = Ocr(); m.submitter = Sub(); m.uid = '1'; m.egg_season = 'S1'; m.reader = Reader(); m.args = type('A', (), {'replay': None, 'cfg': {}})(); m.hold = type('H', (), {'stops': 0, 'start': lambda self, x: None, 'stop': lambda self: setattr(self, 'stops', self.stops + 1)})()
    m.name_reader = lambda: m.reader if reader else None
    m.picture = lambda *a, **k: None; m.egg_hold_end = lambda why: m.__dict__.setdefault('holds', []).append(why); m.screen_settled = lambda *a: None; m.out = lambda *a: None
    m.t = 1000.0
    return m
def show(m, title='Adventure Successful', times=1):
    for _ in range(times):
        m.t += 2; m.egg_result(IM, m.t, U, 0, H, title)
def sent(m):
    return [(p['score'], p['defeats'], p['clearTimeS'], p['cleared']) for p in m.submitter.sent]

m = meter(); show(m); check('one read: nothing goes', sent(m), [])
show(m); check('two alike: it goes', sent(m), [(1234, 1, 132, True)])
check('...the input hold ends then', m.hold.stops, 1)
m.reader.loot = 9999; show(m, times=3); check('later reads make no second record', len(sent(m)), 1)

m = meter(); show(m, 'Adventure'); show(m); check('a title cut short is not an outcome', sent(m), [])
show(m); check('...and there is one record, won', sent(m), [(1234, 1, 132, True)])

m = meter(); show(m, 'Adventure Unsuccessful', 2); check('lost', sent(m)[0][3], False)
m = meter(); show(m, 'Adventure Failed', 2); check('lost, its other words', sent(m)[0][3], False)

m = meter(); show(m); m.reader.loot = None; show(m); m.t += 10; m.egg_gone(m.t)
check('left early: a number one read saw is blank', sent(m), [(None, 1, 132, True)])

m = meter(); show(m); m.egg_gone(m.t + 1); check('still up: nothing yet', sent(m), [])
m = meter(); show(m); m.t += 10; m.egg_gone(m.t); check('one read and gone: nothing agreed, nothing sent', sent(m), [])

m = meter(); vals = iter([10, 20, 30, 40, 50, 60])
for v in vals: m.reader.loot = v; show(m)
check('never agrees: goes after the limit, loot blank', sent(m), [(None, 1, 132, True)])

m = meter(); show(m, times=2); m.t += Meter.EGG_SENT_S + 1; m.reader.loot = 777; show(m, times=2)
check('the next match is its own record', [s[0] for s in sent(m)], [1234, 777])

m = meter(reader=False); m.ocr.extra = {'loot': '4321', 'defeats': 'Defeats: 2'}; show(m); check('no font, one read: nothing', sent(m), [])
show(m); check('no font: OCR, two alike', sent(m), [(4321, 2, 132, True)])

m = meter(reader=False); m.ocr.extra = {'loot': '4UYSY', 'defeats': 'Defeats: 2'}; show(m, times=2); m.t += 10; m.egg_gone(m.t)
check('no font: part of a number is no number', [x[0] for x in sent(m)], [None])

# -- the Party screen with a card that reads blank (B22, B23: a NameError on every such read) ------------------------------
check('the name list reaches live.py', 'mario' in live.INTERLINK_NPCS, True)
sent_party = {}
p = Meter.__new__(Meter); p.character_name = 'Moonpetal'; p.party = None; p.catalog = []
p.party_names = lambda *a: {}; p.ocr = type('O', (), {'read': lambda s, c, **k: {'n0': '1P Moonpetal', 'a0': 'Budling / Lv.60 CP:1000', 'n1': '2P Flora', 'a1': '', 'n2': '3P Someone', 'a2': ''}})()
p.cycle_boss = lambda b: None; p.stage_first = lambda r: r; p.set_party = lambda ms, now: sent_party.update(members=ms); p.read_party_icons = lambda *a: None
results_keep = live.keep_sample; live.keep_sample = lambda *a: None
try:
    p.party_screen(IM, 1000.0, U, 0, H)
finally: live.keep_sample = results_keep
check('a blank card: an AI name is AI, another is not', [(m_['name'], m_['ai']) for m_ in sent_party.get('members', [])], [('Moonpetal', False), ('Flora', True), ('Someone', False)])

# -- Egg Heist's own Party layout, and its party going with the match --------------------------------------------------------
m = Meter.__new__(Meter)
m.party_layout('Operation: Egg Heist Team Mode Normal')
check('its top line: its own layout', (m.egg_party, m.PARTY_CARDS is Meter.EGG_PARTY_CARDS, m.PARTY_SLOTS is Meter.EGG_PARTY_SLOTS), (True, True, True))
m.party_layout('')
check("a line that didn't read: as it was", m.egg_party, True)
m.party_layout('Omega Jeburst')
check("Interlink's: Interlink's layout", (m.egg_party, m.PARTY_CARDS is Meter.EGG_PARTY_CARDS, len(m.PARTY_SLOTS)), (False, False, 4))
check('every egg card is the size of an Interlink one', {(round(c[2] - c[0], 3), round(c[3] - c[1], 3)) for c in Meter.EGG_PARTY_CARDS}, {(0.11, 0.057)})
people = [{'slot': 0, 'name': 'Alpha', 'self': True, 'ai': False, 'aniimo': 'Budling', 'aniimo_level': 70, 'cp': 4000}, {'slot': 1, 'name': 'Bravo', 'self': False, 'ai': False, 'aniimo': 'Cinder', 'cp': 3000}]
m = meter(); m.party = {'at': m.t, 'egg': True, 'members': people, 'teams': [['Dune', None, 'Ember'], ['Frost', 'Glade', None], None, None], 'team': ['Dune', None, 'Ember']}
show(m, times=2)
line = (sent(m), m.submitter.sent[0]['extra']['lineup'])[1]
check('the match has its party', [(p['name'], p['aniimo']) for p in line], [('Alpha', 'Budling'), ('Bravo', 'Cinder')])
check('...and their teams', [t['aniimo'] for t in line[1]['team']], ['Cinder', 'Frost', 'Glade'])
m.t += Meter.EGG_SENT_S + 1; m.reader.loot = 777; show(m, times=2)
check('the next match has no old party', (sent(m), 'lineup' in m.submitter.sent[1]['extra'])[1], False)
m = meter(); m.party = {'at': m.t, 'members': people}; show(m, times=2)
check("Interlink's party is not a match's", (sent(m), 'lineup' in m.submitter.sent[0]['extra'])[1], False)
m = meter(); m.party = {'at': m.t - Meter.EGG_PARTY_S - 1, 'egg': True, 'members': people}; show(m, times=2)
check('too long ago', (sent(m), 'lineup' in m.submitter.sent[0]['extra'])[1], False)

# -- Egg Heist's Party screen: three pills; its rank screen over it ------------------------------------------------------------
seen = {}
def reads(crops, names=None):
    missing = [k for k in (names or {}) if k not in crops]         # the real reader's KeyError, 8 Nov
    if missing: raise KeyError(missing[0])
    return {'n0': '1P Moonpetal', 'a0': 'Budling / Lv.60 CP:1000', 'n1': '2P Bravo', 'a1': 'Cinder / Lv.60 CP:900', 'n2': '', 'a2': ''}
p = Meter.__new__(Meter); p.character_name = 'Moonpetal'; p.party = None; p.catalog = []; p.ocr = type('O', (), {'read': lambda s, c, names=None: reads(c, names)})()
p.party_layout('Operation: Egg Heist Team Mode'); p.cycle_boss = lambda b: None; p.stage_first = lambda r: r
p.set_party = lambda ms, now: seen.update(members=ms); p.read_party_icons = lambda *a: None
live.keep_sample = lambda *a: None
crops3 = {f'n{i}': IM for i in range(3)}
p.party_names = lambda *a: crops3
p.party_screen(IM, 1000.0, U, 0, H)
check("three pills: it's read, no fourth asked for", [m_['name'] for m_ in seen.get('members', [])], ['Moonpetal', 'Bravo'])
p.ocr = type('O', (), {'read': lambda s, c, names=None: {'n0': '1P Moonpetal', 'a0': 'Budling / Lv.60 CP:1000', 'n1': '', 'a1': '', 'n2': 'Xq38', 'a2': ''}})(); p.party_seen = 0; seen.clear()
p.party_screen(IM, 2000.0, U, 0, H)
check("an empty spot's specks aren't a player", [m_['name'] for m_ in seen.get('members', [])], ['Moonpetal'])
rank = {'h': 'Party', 'ph': 'Operation: Egg Heist Team Mode', 'ep': '1464/1530'}
check('the rank screen over it is the rank screen', Meter.screen_spotted(rank), 'Egg Heist')
q = Meter.__new__(Meter); q.fight = None
check('...not the Party screen', q.party_up(rank, 100.0), False)
# -- the rank screen: its line, both layouts, only after a match's end screen ------------------------------------------------------
check('its line spots it', Meter.screen_spotted({'et': 'Tap anywhere to continue'}), 'Egg Heist')
check("a controller's wording too", Meter.rank_line({'et': 'Press A to continue'}), True)
check('another line does not', Meter.rank_line({'et': 'Continue the story'}), False)
g = Meter.__new__(Meter)
up = {'h': 'x', 'et': 'Tap anywhere to continue', 'ep': '1464/1530', 'es': 'x5'}
check('no end screen before it: not the rank screen', g.rank_only_after_match(up, 5000.0), {'h': 'x'})
g.egg_end_seen = 4950.0
check('just after an end screen: it is', g.rank_only_after_match(up, 5000.0), up)
g.egg_end_seen = 4800.0; g.egg_sent_at = 4800.0
check('long after: not any more', g.rank_only_after_match(up, 5000.0), {'h': 'x'})
class Reads:
    def __init__(s, high, low): s.high, s.low, s.calls = high, low, 0
    def read(s, crops, names=None):
        s.calls += 1
        if 'high' in crops: return {'high': s.high, 'low': s.low}
        return {'title': 'Pro Egg Snatcher Ill', 'title2': '', **{name: '+40' for name in crops if name.startswith('gain')}}
for what, high, low in (('the higher layout', '1464/1530', '15111 IOZU'), ('the lower layout', '', '1464/1530')):
    g = Meter.__new__(Meter); g.ocr = Reads(high, low); g.uid = '302000000001'; g.submitter = None; g.last_scores = {}; g.out = lambda *a: None
    rank_seen = []; g.egg_rank_seen = lambda im, now: rank_seen.append(now)
    pill = Image.new('RGB', (U, H), (40, 30, 60))
    pill.paste((255, 225, 80), (int(U * 0.87), int(H * 0.55), int(U * 0.905), int(H * 0.58)))
    g.egg_rank_gain(pill, 100.0, U, 0, H); check(f'{what}: nothing on one read', rank_seen, [])
    g.egg_rank_gain(pill, 100.4, U, 0, H); check(f'{what}: two alike and it is read', rank_seen, [100.4])
    check(f'{what}: the gain and total', g.last_scores.get('egg_gain', (None, None, None))[1:], (1464, 40))
g = Meter.__new__(Meter); g.ocr = Reads('1500/1530', ''); g.egg_points = '1464/1530'; rank_seen = []; g.egg_rank_seen = lambda im, now: rank_seen.append(now)
g.egg_rank_gain(pill, 100.0, U, 0, H); check('still counting up: not read', rank_seen, [])
# -- a quick press: the rank screen leaves before its second read ------------------------------------------------------------------------
class Whole:
    def __init__(s, seen): s.seen = seen
    def __call__(s): return s.seen
def fast(points='1464/1530'):
    g = Meter.__new__(Meter); g.ocr = Reads(points, ''); g.uid = '302000000001'; g.submitter = None; g.last_scores = {}; g.out = lambda *a: None
    got = []; g.egg_rank_seen = lambda im, now: (got.append(now), setattr(g, 'egg_rank_read_at', now))
    return g, got
up = {'et': 'Tap anywhere to continue'}; gone = {'h': 'x'}
g, got = fast(); g.egg_rank_watch(pill, up, 100.0); g.egg_rank_gain(pill, 100.0, U, 0, H); g.egg_rank_watch(pill, gone, 100.2)
check('one read that missed it: not gone yet', got, [])
g.egg_rank_watch(pill, gone, 100.6)
check('gone after one read: the kept frame is read', (len(got), g.last_scores['egg_gain'][1:], g.egg_rank_frame, g.egg_points), (1, (1464, 40), None, None))
g, got = fast(); g.egg_rank_watch(pill, up, 100.0); g.egg_rank_gain(pill, 100.0, U, 0, H); g.egg_rank_watch(pill, up, 100.3); g.egg_rank_gain(pill, 100.3, U, 0, H)
g.egg_rank_watch(pill, gone, 101.0); check('read twice while up: not read again when it goes', got, [100.3])
parts = pill.copy(); parts.info['parts'] = True
g, got = fast(); g.egg_rank_watch(parts, up, 100.0); g.egg_rank_watch(pill, gone, 101.0); check('only a frame of parts: nothing to read', (got, g.egg_rank_frame), ([], None))
g, got = fast(); g.egg_points = '1464/1530'; g.egg_rank_watch(pill, gone, 100.0); check('a leftover first read goes, so fast reads stop', g.egg_points, None)
g, got = fast(); g.egg_rank_watch(pill, up, 100.0); g.egg_rank_watch(pill, gone, 120.0); check('gone long ago: too old to read', got, [])
# -- 4 reads a second only while an Egg Heist screen is being read -------------------------------------------------------------------------
def due(after, **state):
    p_ = Meter.__new__(Meter); p_.last_results = 100.0; p_.auto_active = lambda: False; p_.idle = lambda: False
    p_.top_sig = p_.screen_read_sig = b'same'; live.screen_changed = lambda a, b, limit=24: False
    for key, value in state.items(): setattr(p_, key, value)
    return p_.screen_read_due(100.0 + after)
keep_changed = live.screen_changed
check('a still screen: every 1.5 s', (due(0.3), due(1.6)), (False, True))
for key, value in (('egg_hold', [0, 0, 0]), ('egg_read', {'votes': {}}), ('egg_points', '1/2')): check(f'{key}: 4 a second', (due(0.2, **{key: value}), due(0.3, **{key: value})), (False, True))
live.screen_changed = keep_changed
# -- gains and losses ---------------------------------------------------------------------------------------------------------------
import eggheist
for text, want in (('+40', 40), ('-12', -12), ('\u221212', -12), ('', None), ('40', None)): check(f'the pill {text!r}', eggheist.parse_gain(text), want)
class NoPill(Reads):
    def read(s, crops, names=None):
        s.calls += 1
        return {'high': s.high, 'low': s.low} if 'high' in crops else {'title': 'Pro Egg Snatcher Ill', 'title2': ''}
results.HERE, keep_here = tempfile.mkdtemp(prefix='alogs_egg_'), results.HERE; os.makedirs(os.path.join(results.HERE, 'logs'))
for before, seen, want in ((1600, 1577, -23), (1537, 1577, 40), (1577, 1577, None)):
    g = Meter.__new__(Meter); g.ocr = NoPill(f'{seen}/1620', ''); g.uid = f'30200000{before}'; g.egg_season = 'S1'; g.submitter = None; g.last_scores = {}
    told = []; g.out = lambda *a: told.append(a[1]); g.egg_rank_seen = lambda im, now: None
    g.egg_profile('S1', bar=[before, 1620])
    g.egg_rank_gain(IM, 100.0, U, 0, H); g.egg_rank_gain(IM, 100.4, U, 0, H)
    check(f'no pill read, {before} to {seen}: the change', g.last_scores['egg_gain'][2], want)
    check(f'...and it says so', (want is None) or (f'{want:+}' in told[-1]), True)
results.HERE = keep_here
# -- the end screen's Next -----------------------------------------------------------------------------------------------------------
class Labels:
    def __init__(s, **seen): s.seen = seen
    def read(s, crops, names=None): return {k: s.seen.get(k, '') for k in crops}
clicks = []; keep_click, keep_front = live.wc.click_at, live.wc.is_foreground
live.wc.click_at = lambda x, y: clicks.append((round(x), round(y))); live.wc.is_foreground = lambda h: front[0]
front = [True]
def nexter(**seen):
    n = Meter.__new__(Meter); n.ocr = Labels(**seen); n.hwnd = 1; n.origin = (0, 0); n.hold = type('H', (), {'active': False})()
    n.egg_click = {'at': 100.0, 'last': 0.0, 'tries': 0}; return n
n = nexter(controller='Next'); n.egg_next(IM, 100.0, U, 0, H); check('the higher button', clicks, [(round(U * 0.88), round(H * 0.879))]); clicks.clear()
n = nexter(keyboard='Next'); n.egg_next(IM, 100.0, U, 0, H); check('the lower button', clicks, [(round(U * 0.88), round(H * 0.9144))]); clicks.clear()
n = nexter(controller='Share'); n.egg_next(IM, 100.0, U, 0, H); check('not reading Next: no press', clicks, [])
n.egg_next(IM, 100.5, U, 0, H); check('...not tried again at once', n.egg_click['tries'], 1)
for t_ in (102.0, 104.0, 106.0, 108.0): n.egg_next(IM, t_, U, 0, H)
check('...and it gives up', (clicks, n.egg_click), ([], None))
front[0] = False; n = nexter(controller='Next'); n.egg_next(IM, 100.0, U, 0, H); check('game not in front: no press, and none later', (clicks, n.egg_click), ([], None)); front[0] = True
n = nexter(controller='Next'); n.egg_next(IM, 200.0, U, 0, H); check('long after: no press', (clicks, n.egg_click), ([], None))
live.wc.click_at, live.wc.is_foreground = keep_click, keep_front
check('the Party screen by its top line', q.party_up({'h': '', 'ph': 'Operation: Egg Heist Team Mode'}, 100.0), True)

# -- the match goes at once; the rank screen updates it --------------------------------------------------------------------------
import shutil
folder = os.path.join(live.HERE, 'logs', 'screens'); os.makedirs(folder, exist_ok=True)
def real_picture(im, where, name, proof=False, fresh=True):
    path = os.path.join(folder, name + '.jpg'); open(path, 'wb').write(b'x'); return path
m = meter(); m.picture = real_picture; show(m, times=2)
check('read: it goes at once', len(m.submitter.sent), 1)
first = m.submitter.sent[0]['clientRunId']; kept = m.egg_pending['shot']
check('a copy waits for the rank screen', os.path.exists(os.path.join(live.HERE, kept)), True)
m.egg_rank_seen(IM, m.t + 5)
check('the rank screen updates the same record', [p['clientRunId'] for p in m.submitter.sent], [first, first])
check('...with its picture', 'egg_rank' in m.submitter.files[1][1], True)
check('...and the end screen again', m.submitter.files[1][0], kept)
m = meter(); m.picture = real_picture; show(m, times=2); kept = m.egg_pending['shot']
m.egg_gone(m.t + 10); check('not long after: still waiting', bool(m.egg_pending), True)
m.egg_gone(m.t + Meter.EGG_RANK_WAIT_S + 1)
check('no rank screen came: nothing more sent', len(m.submitter.sent), 1)
check('...and the copy is gone', os.path.exists(os.path.join(live.HERE, kept)), False)
class Hold:
    started = 0
    def start(self, s): Hold.started += 1
    def stop(self): pass
m = meter(); m.hold = Hold(); m.args = type('A', (), {'replay': None, 'cfg': {'leaderboard': {'submit': True, 'automation': True}, 'results': {'egg_fade_hold': True}}})(); m.hwnd = None; m.space = 'x'
m.egg_screen_hold('Egg Heist', {'eh': 'Adventure Successful'}, 100.0); check('an end screen: inputs held', Hold.started, 1)
m.egg_hold = None; m.egg_sent_at = 100.0
m.egg_screen_hold('Egg Heist', {'eh': 'Adventure Successful'}, 110.0); check('...not once it went', Hold.started, 1)
m.egg_screen_hold('Egg Heist', {'ep': '1464/1530'}, 120.0); check('its rank screen: held', Hold.started, 2)
m.egg_hold = None; m.egg_sent_at = 0
m.egg_screen_hold('Egg Heist', {'ep': '18/18'}, 9000.0); check('a rank-looking number with no match before: not held', Hold.started, 2)
m.egg_screen_hold('party', {}, 9000.0); check('other screens: not held', Hold.started, 2)

# -- any captured result: inputs held until it's sent, then back; a 10 s failsafe -----------------------------------------------
class Holder:
    def __init__(self): self.seconds = []; self.stops = 0
    def start(self, s): self.seconds.append(s)
    def stop(self): self.stops += 1
def sender(automation=True, hold=True):
    m = meter(); m.hold = Holder(); m.hwnd = None; m.submitter = Sub()
    m.args = type('A', (), {'replay': None, 'cfg': {'leaderboard': {'submit': True, 'automation': automation}, 'results': {'hold_inputs': hold}}})()
    return m
m = sender(); m.send_result({'clientRunId': 'a'}, None)
check('held for the failsafe', m.hold.seconds, [10])
m.result_sent({'clientRunId': 'other'}); check("another record's sending doesn't let go", m.hold.stops, 0)
m.result_sent({'clientRunId': 'a'}); check('sent: inputs back', m.hold.stops, 1)
m.result_sent({'clientRunId': 'a'}); check('...once', m.hold.stops, 1)
m = sender(automation=False); m.send_result({'clientRunId': 'a'}, None); check('no automation: never held', m.hold.seconds, [])
check('...but still sent', len(m.submitter.sent), 1)
m = sender(hold=False); m.send_result({'clientRunId': 'a'}, None); check('holding switched off: never held', m.hold.seconds, [])
m = sender(); m.egg_pending = {'payload': {'clientRunId': 'e'}, 'shot': None, 'party': None}; m.egg_update('rank.jpg')
check('a match too', (m.hold.seconds, m.submitter.files[0][1]), ([10], 'rank.jpg'))

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('egg result reads: ok')
