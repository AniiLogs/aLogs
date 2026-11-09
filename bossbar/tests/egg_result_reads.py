"""Egg Heist's end screen: the game's digits, two reads alike, one record per match.

    python tests/egg_result_reads.py

Meter.egg_result (8 Nov: Loot 40939 went as 4, Total Defeats blank, and one match went twice, once as "Adventure"):
- nothing goes on one read, and a number that only one read saw isn't sent;
- a title that isn't clearly won or lost isn't an outcome, and doesn't make a second record;
- later reads of the same screen, even with another number, make no second record;
- a screen that leaves early sends what agreed, with the rest blank;
- a screen that never agrees goes after EGG_MAX_READS, with the rest blank;
- without the game's font the OCR read is used, still two alike, and only when all of it is a number.
Meter.party_screen: a card that reads blank doesn't stop the read (INTERLINK_NPCS reaches live.py).
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
    def enqueue(self, payload, shot): self.sent.append(payload)
def meter(reader=True):
    m = Meter.__new__(Meter); m.ocr = Ocr(); m.submitter = Sub(); m.uid = '1'; m.egg_season = 'S1'; m.reader = Reader()
    m.name_reader = lambda: m.reader if reader else None
    m.picture = lambda *a, **k: None; m.egg_hold_end = lambda why: m.__dict__.setdefault('holds', []).append(why); m.screen_settled = lambda *a: None; m.out = lambda *a: None
    m.t = 1000.0
    return m
def show(m, title='Adventure Successful', times=1):
    for _ in range(times):
        m.t += 2; m.egg_result(IM, m.t, U, 0, H, title)
def sent(m): return [(p['score'], p['defeats'], p['clearTimeS'], p['cleared']) for p in m.submitter.sent]

m = meter(); show(m); check('one read: nothing goes', sent(m), [])
show(m); check('two alike: it goes', sent(m), [(1234, 1, 132, True)])
check('...the input hold ends then', len(m.holds), 1)
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

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('egg result reads: ok')
