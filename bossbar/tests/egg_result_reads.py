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
