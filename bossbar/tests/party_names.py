"""The Party screen's names don't overwrite good result-screen and Battle Data reads.

    python tests/party_names.py

Each result-screen (and Battle Data) column takes the Party screen's player of its slot (live.party_slots), but:
- your row keeps your name and the mark as you when it reads as your name (the game menu showed it), whatever the Party
  screen read there: on its other layout (pills higher, before everyone is ready) it reads the title or half a pill;
- no second row is marked as you, nor takes the Party screen's you, nor your name read unmarked in another slot;
- each Party member goes to one column at most: a column that clearly reads a member's name (0 as O, 1 as l) takes
  them first, and a column whose slot's member went to another keeps its own read, with no AI flag from the party;
- a one-character name the Korean model vouched for (its read in the name cache for good: live.sure_read) stays;
- an ordinary misread ('Dyn*ht') still takes the Party screen's name, and with no Party read names stay as read.
Made-up players; the data folder is a temporary one. Exit code 1 on any failure.
"""
import os, shutil, sys, tempfile
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
TMP = tempfile.mkdtemp(prefix='alogs_party_')
os.environ['ALOGS_DATA'] = TMP
from PIL import Image
import live, reader as R

fails = []
def check(what, get, want):
    try: got = get()
    except Exception as e: got = f'{type(e).__name__}: {e}'
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

YOU = 'Moonpetal'
live.TEAM.seen = {'Wexley': 4}                      # a teammate read clearly on 4 Party screens before
def member(slot, name, me=False, ai=False): return {'slot': slot, 'name': name, 'self': me, 'ai': ai}
def result(party, reads, sure=(), you=YOU):
    """The result screen's players after party_slots and mark_you, as result_screen leaves them: (name, you, AI)."""
    players = [{'slot': s, 'name': n, 'stat': 'Damage', 'value': 1000 + s} for s, n in reads.items()]
    live.party_slots(players, party, sure=sure, you=you)
    live.mark_you(players, party, you)
    return [(p['name'], p['self'], p['ai']) for p in players]

# your row on the Party screen's other layout: its slot read the title above, unmarked. Your row stays you
party = [member(0, 'Vaultkeep'), member(1, 'Brannoch'), member(2, 'Coolio')]
check('your row read on the other layout', lambda: result(party, {0: YOU, 1: 'Brannoch', 2: 'Coolio'}),
      [(YOU, True, False), ('Brannoch', False, False), ('Coolio', False, False)])
check('your row read a letter off', lambda: result(party, {0: 'Moonpeta1', 1: 'Brannoch', 2: 'Coolio'})[0], (YOU, True, False))

# the Party screen put you in slot 3; the result screen reads you in column 1. Column 3 isn't you too
party = [member(0, 'Zarvok'), member(2, YOU, me=True), member(3, 'Quillby')]
check('no second you (Party you elsewhere)', lambda: result(party, {0: YOU, 1: '', 2: 'Ashgrove', 3: 'Quillby'}),
      [(YOU, True, False), ('', False, False), ('Ashgrove', False, False), ('Quillby', False, False)])
# its mark on slot 1 and your name unmarked on slot 2 (a slot keeps its commonest name, the mark is the last read's)
party = [member(0, 'Zarvok', me=True), member(1, YOU), member(3, 'Quillby')]
check('no second you (your name unmarked)', lambda: result(party, {0: YOU, 1: 'Ashgrove', 3: 'Quillby'}),
      [(YOU, True, False), ('Ashgrove', False, False), ('Quillby', False, False)])

# column 4 clearly reads slot 2's AI teammate: it takes her, and column 2 keeps its own read (no AI flag from her),
# through the teammates read before ('Wex1ey' is Wexley)
party = [member(0, YOU, me=True), member(1, 'Flora', ai=True), member(2, 'Coolio'), member(3, 'Ashgrove')]
check('each member to one column', lambda: result(party, {0: YOU, 1: 'Wex1ey', 2: '', 3: 'Flora'}),
      [(YOU, True, False), ('Wexley', False, False), ('Coolio', False, False), ('Flora', False, True)])
# a read with 1 for l is slot 3's player (folded): column 4 takes him, so he isn't listed twice under two spellings
party = [member(0, YOU, me=True), member(1, 'Brannoch'), member(2, 'Coolio')]
check('a member read with 1 for l', lambda: [n for n, _, _ in result(party, {0: YOU, 1: 'Brannoch', 2: 'Ashgrove', 3: 'Coo11o'})],
      [YOU, 'Brannoch', 'Ashgrove', 'Coolio'])

# a one-character name: the Party screen never keeps one, so its slot holds a misread ('Kxq'). Kept when vouched for
party = [member(0, YOU, me=True), member(1, 'Brannoch'), member(3, 'Kxq')]
check('vouched one-character name', lambda: result(party, {0: YOU, 1: 'Brannoch', 3: 'K'}, sure={3})[2], ('K', False, False))
check('unsure one-character read', lambda: result(party, {0: YOU, 1: 'Brannoch', 3: 'K'})[2], ('Kxq', False, False))
box = Image.new('L', (120, 30), 40)
class Cache:
    def __init__(self, until): self.name_cache = {R.name_signature(box): ('K', until)}
check('sure_read: vouched (cached for good)', lambda: live.sure_read(Cache(None), box), True)
check('sure_read: unsure (cached for a while)', lambda: live.sure_read(Cache(123.0), box), False)
check('sure_read: not cached', lambda: live.sure_read(Cache(None), Image.new('L', (60, 30), 200)), False)
check('sure_read: no name cache', lambda: live.sure_read(object(), box), False)

# an ordinary misread still takes the Party screen's name; with no Party read, names stay as read (teammates aside)
party = [member(0, YOU, me=True), member(1, 'Dynlight'), member(2, 'Coolio')]
check('ordinary misread', lambda: result(party, {0: YOU, 1: 'Dyn*ht', 2: 'Coolio'}),
      [(YOU, True, False), ('Dynlight', False, False), ('Coolio', False, False)])
check('no party read', lambda: result(None, {0: YOU, 1: 'Wex1ey', 2: 'Zarvok'}),
      [(YOU, True, False), ('Wexley', False, False), ('Zarvok', False, False)])
# Battle Data cards go through party_slots too: no card listed twice
cards = [{'slot': s, 'name': n, 'avg_dps': 1000} for s, n in enumerate([YOU, 'Wex1ey', '', 'Flora'])]
party = [member(0, YOU, me=True), member(1, 'Flora', ai=True), member(2, 'Coolio')]
check('Battle Data cards', lambda: (live.party_slots(cards, party, you=YOU), [c['name'] for c in cards])[1], [YOU, 'Wexley', 'Coolio', 'Flora'])

shutil.rmtree(TMP, ignore_errors=True)
if fails:
    print(f'{len(fails)} FAILED:'); [print('  ' + f) for f in fails]; sys.exit(1)
print("Party names: your row stays you, each member goes to one column, vouched one-character names stay")
