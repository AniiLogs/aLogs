"""A result-screen slot whose stat label was read is a player, even when its name and value weren't.

    python tests/result_slots.py

Kept only when its name or value was read, such a slot dropped out, and that player and their Battle Data card weren't
sent (5 sent runs, 31 Oct-1 Nov). Now it is kept and takes the Party screen's player of its slot. A slot with nothing
read, or only text that isn't a stat label, stays out: no phantom player in a solo, AI or 2-3 player run. The result
screen is read through result_screen with a stand-in OCR on a blank picture; made-up players, and the data folder is a
temporary one. Exit code 1 on any failure.
"""
import ast, logging, os, re, shutil, sys, tempfile, types
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
TMP = tempfile.mkdtemp(prefix='alogs_slots_')
os.environ['ALOGS_DATA'] = TMP
from PIL import Image
import live

fails = []
def check(what, get, want):
    try: got = get()
    except Exception as e: got = f'{type(e).__name__}: {e}'
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

class Capture(logging.Handler):
    def __init__(self): super().__init__(); self.lines = []
    def emit(self, record): self.lines.append(record.getMessage())
cap = Capture(); logging.getLogger('meter').addHandler(cap); logging.getLogger('meter').setLevel(logging.INFO)

class Ocr:
    """Stands in for the helper: each box reads what the case says."""
    name_cache = {}
    def __init__(self, reads): self.reads = reads
    def read(self, crops, **_): return {k: self.reads.get(k, '') for k in crops}

def member(slot, name, me=False, ai=False): return {'slot': slot, 'name': name, 'self': me, 'ai': ai}

def players(reads, party=None):
    """The result screen's players as result_screen leaves them: (slot, name, stat, value)."""
    m = live.Meter.__new__(live.Meter)
    m.args = types.SimpleNamespace(duration=180, cfg={})
    m.ocr = Ocr({'rating': '30', 'score': '30123', 'time': '180s', **reads}); m._name_reader = None
    m.result_seen = 0.0; m.fight = None; m.last_fight = None; m.uid = None; m.character_name = None
    m.submitter = None; m.party = {'at': 1000.0, 'members': party} if party else None
    m.picture = lambda *a, **k: None; m.out = lambda *a, **k: None
    cap.lines.clear()
    m.result_screen(Image.new('RGB', (1920, 1080)), 1000.0, 1920, 0, 1080, 'Battle Complete')
    line = next(l for l in cap.lines if 'result screen read' in l)
    return [(p['slot'], p['name'], p['stat'], p['value']) for p in ast.literal_eval(re.search(r'players (\[.*\])$', line).group(1))]

def slot(i, name, stat, value=''): return {f'name{i}': name, f'stat{i}': f'{stat} / {value}' if value else stat}
FOUR = [member(0, 'Vaultkeep'), member(1, 'Brannoch'), member(2, 'Coolio'), member(3, 'Ashgrove')]

# 3P's name and value unread, its label read: it stays, as the Party screen's 3P
reads = {**slot(0, 'Vaultkeep', 'Total Damage Dealt', '912345'), **slot(1, 'Brannoch', 'Most Healing', '23456'),
         **slot(2, '', 'Total BREAK Damage'), **slot(3, 'Ashgrove', 'Targets Defeated', '2')}
check('label only: kept as its slot\'s player', lambda: players(reads, FOUR)[2], (2, 'Coolio', 'Total BREAK Damage', None))
check('label only: four players', lambda: [p[0] for p in players(reads, FOUR)], [0, 1, 2, 3])
# no Party screen read: still a player, with no name (its Battle Data card names it when it goes)
check('label only, no party', lambda: players(reads)[2], (2, '', 'Total BREAK Damage', None))
# a label misread a little ('Tota1 BREAK Darnage') is still the label
check('label misread', lambda: players({**reads, **slot(2, '', 'Tota1 BREAK Darnage')}, FOUR)[2][2], 'Total BREAK Damage')
# 'Best Strike Damage' is a stat label too (the game's text table; 321 slot reads of 31 Oct-1 Nov carry it): a slot
# with only that label read stays, and a misread of it ('Best Str1ke Darnage') snaps to the game's wording
check('label only: Best Strike Damage', lambda: players({**reads, **slot(2, '', 'Best Strike Damage')}, FOUR)[2],
      (2, 'Coolio', 'Best Strike Damage', None))
check('Best Strike Damage misread', lambda: players({**reads, **slot(2, '', 'Best Str1ke Darnage')}, FOUR)[2][2],
      'Best Strike Damage')

# solo: three empty slots read nothing, no phantom players
solo = slot(0, 'Vaultkeep', 'Total Damage Dealt', '1912345')
check('solo: one player', lambda: [p[0] for p in players(solo, FOUR[:1])], [0])
# an AI teammate and an empty slot: two players and the AI, no fourth
three = {**solo, **slot(1, 'Flora', 'Most Healing', '5432'), **slot(2, 'Brannoch', 'Most Damage Taken', '76543')}
check('2 players and an AI: three', lambda: [p[0] for p in players(three, [FOUR[0], member(1, 'Flora', ai=True), FOUR[1]])], [0, 1, 2])
# text in an empty slot's label box that isn't a stat label stays out
check('not a label: no player', lambda: [p[0] for p in players({**solo, 'stat2': 'ffl', 'stat3': 'i'}, FOUR[:1])], [0])

shutil.rmtree(TMP, ignore_errors=True)
if fails:
    print(f'{len(fails)} FAILED:'); [print('  ' + f) for f in fails]; sys.exit(1)
print('result slots: a slot with its stat label read is a player; an empty slot never is')
