"""A name the result screen or Battle Data read alike again and again beats the Party screen's read of it.

    python tests/settled_names.py

live.party_slots with `agreed`, submit._lineup and Meter.party_screen (8 Nov: 3 of 4 names sent were the Party screen's
misreads of names the result screen read right 5 times, all four cards went without a lead Aniimo, and yours was misread):
- an agreed read stays, with the member's AI flag and you when it's alike; one nothing like the member is someone else,
  with neither;
- I, l and 1 can't be told apart, so those go by the Party screen's spelling, and no name flips between runs;
- a lone read, or none, still gives way to the Party screen, and your own column still reads as you;
- agreed_slots asks for two reads the same: a fading screen's one 'Brannoch556' doesn't count;
- a run's cards go by the settled names, except a name nothing like the card's;
- a card keeps its lead Aniimo though its name reads differently, while its CP stays; a new CP is a new lead;
- your card has your known name.
Made-up names; the OCR is stubbed. Exit code 1 on any failure.
"""
import collections, os, sys, tempfile
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_settled_')
from PIL import Image
import live, results, submit
from live import Meter

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

def member(slot, name, **k): return {'slot': slot, 'name': name, 'self': False, 'ai': False, **k}
def row(slot, name): return {'slot': slot, 'name': name, 'stat': 'Damage', 'value': 1}
def names(players): return [(p['name'], p.get('ai'), p.get('self')) for p in players]
party = [member(0, 'rrannocn'), member(1, 'Moonpetal', self=True), member(2, 'Quillbx', ai=True), member(3, 'Vaultkeep')]

p = [row(0, 'Brannoch'), row(1, 'Moonpetal'), row(2, 'Quillby'), row(3, 'Someone')]
live.party_slots(p, party, you='Moonpetal', agreed={0, 1, 2, 3})
check('agreed reads', names(p), [('Brannoch', False, False), ('Moonpetal', False, True), ('Quillby', True, False), ('Someone', False, False)])

p = [row(0, 'Brannoch556'), row(1, 'Moonpetal'), row(2, ''), row(3, 'Vaultkep')]
live.party_slots(p, party, you='Moonpetal', agreed={3})
check('lone reads give way', names(p), [('rrannocn', False, False), ('Moonpetal', False, True), ('Quillbx', True, False), ('Vaultkep', False, False)])

p = [row(0, 'x'), row(1, 'y')]
live.party_slots(p, party, agreed=())
check('nothing agreed: as before', [q['name'] for q in p], ['rrannocn', 'Moonpetal'])

p = [row(0, 'lKar'), row(1, 'Coolit')]; twin = [member(0, 'IKar'), member(1, 'CoolIt')]
live.party_slots(p, twin, agreed={0, 1})
check('I, l and 1 go by the Party; a case slip does not', [q['name'] for q in p], ['IKar', 'Coolit'])

votes = {}
for read in ('Brannoch556', 'Brannoch', 'Brannoch'): results.vote(votes, 'name0', read)
results.vote(votes, 'name1', 'Once')
check('agreed_slots', results.agreed_slots(votes, lambda i: f'name{i}'), {0})

summary = {'party': [member(0, 'rrannocn'), member(1, 'Zarvok', aniimo='Blazen'), member(2, 'Moonpeta1', self=True)],
           'result': {'players': [{'slot': 0, 'name': 'Brannocx'}, {'slot': 1, 'name': 'Ashgrove'}, {'slot': 2, 'name': 'Moonpetal'}]}}
check('cards go by settled names', [e['name'] for e in submit._lineup(summary)['lineup']], ['Brannocx', 'Zarvok', 'Moonpetal'])
check('no result: as read', [e['name'] for e in submit._lineup({'party': summary['party']})['lineup']], ['rrannocn', 'Zarvok', 'Moonpeta1'])

# -- the Party screen's own read ------------------------------------------------------------------------------------
class Ocr:
    reads = []
    def read(self, crops, **k): return self.reads.pop(0)
m = Meter.__new__(Meter); m.ocr = Ocr(); m.party = None; m.character_name = 'Moonpetal'; m.party_seen = 0; m.catalog = []
m.screen_settled = lambda *a: None; m.read_party_icons = lambda *a: None; m.cycle_boss = lambda b: None; m.stage_first = lambda r: r
m.party_names = lambda *a: {}
im = Image.new('RGB', (1280, 720), (40, 60, 90))
def read(now, n0, a0, n2='3P Moonpeta1'):
    Ocr.reads = [{'n0': n0, 'a0': a0, 'n2': n2, 'a2': 'Fulmintis / Lv.70 CP:4006', 'boss': ''}]
    m.party_seen = 0
    m.party_screen(im, now, 1280, 0, 720)
    return next(x for x in m.party['members'] if x['slot'] == 0), next(x for x in m.party['members'] if x['slot'] == 2)
first, me = read(100, '1P Brannoch', 'Blazen / Lv.70 CP:3327')
check('first read', first['aniimo'], 'Blazen')
check('your card has your name', (me['name'], me['self']), ('Moonpetal', True))
kept, _ = read(103, '1P rrannocn', 'Lv.70 CP:3327')
check('another name, same CP: lead kept', kept['aniimo'], 'Blazen')
gone, _ = read(106, '1P Brannoch', 'Lv.70 CP:2000')
check('a new CP: a new lead', gone['aniimo'], None)

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('settled names: ok')
