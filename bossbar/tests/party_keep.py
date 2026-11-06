"""A Party screen read that misses a player doesn't drop them; two misses in a row do.

    python tests/party_keep.py

Meter.set_party: on 6 Nov the last read before the fight caught 3 of 4 names (one of them yours, misread), and the run
went with 3 players and no "you". Now a player missing from one read is kept (with their card's details), your
misread name still counts as you, and a player who really left is gone after a second read without them.
Made-up names only. Exit code 1 on any failure.
"""
import os, sys, tempfile
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_party_')
import live

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

def member(slot, name, me=False):
    return {'slot': slot, 'name': name, 'self': me, 'ai': False, 'pill': slot, 'aniimo': f'Pet{slot}', 'cp': 3000 + slot}

m = live.Meter.__new__(live.Meter)
m.party = None
m.screen_settled = lambda kind, now: None
full = lambda: [member(0, 'Alpha', True), member(1, 'Bravo'), member(2, 'Charlie'), member(3, 'Delta')]
names = lambda: [x['name'] for x in m.party['members']]

for t in (100, 103, 106):                          # the screen read a few times, all four there
    m.set_party(full(), t)
check('four read', names(), ['Alpha', 'Bravo', 'Charlie', 'Delta'])

m.set_party([member(0, 'AIpha'), member(2, 'Charlie'), member(3, 'Delta')], 109)    # one read misses Bravo, misreads you
check('a miss keeps the player', names(), ['Alpha', 'Bravo', 'Charlie', 'Delta'])
check('...with their card', m.party['members'][1].get('aniimo'), 'Pet1')
check('your misread name is still you', m.party['members'][0]['self'], True)

m.set_party(full(), 112)                           # back to normal
check('a good read after', names(), ['Alpha', 'Bravo', 'Charlie', 'Delta'])

m.set_party([member(0, 'Alpha', True), member(1, 'Bravo'), member(2, 'Charlie')], 115)   # Delta leaves
check('one read without them: kept', names(), ['Alpha', 'Bravo', 'Charlie', 'Delta'])
m.set_party([member(0, 'Alpha', True), member(1, 'Bravo'), member(2, 'Charlie')], 118)
check('two reads without them: gone', names(), ['Alpha', 'Bravo', 'Charlie'])

m.party = None                                     # a new party a while later starts from what it reads
m.set_party([member(0, 'Alpha', True), member(1, 'Echo')], 1000)
check('a new party', names(), ['Alpha', 'Echo'])

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('party keep: ok')
