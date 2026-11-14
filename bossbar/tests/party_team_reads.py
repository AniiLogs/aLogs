"""Each player's team from the Party screen follows the card it was read from, never the last run's.

    python tests/party_team_reads.py

Meter.read_party_icons / set_party: on 7 Nov a 2-of-3 vote per tile kept the previous lineup's reads, and runs read
only once or twice before the fight sent the last run's teams (13 tiles of 141 wrong). Now:
- the newest read of a tile stands; a tile a read couldn't make out keeps the last read of the same card;
- a card whose player or lead Aniimo changed starts over: no old team, even before it's read again;
- a pill misread for a moment (the slot list differs) keeps the other cards' teams;
- a read that was running while a card changed hands isn't put on the new player's card;
- a frame that comes while a read runs is read right after it.
Made-up names; the tile reader is stubbed (no pictures). Exit code 1 on any failure.
"""
import os, sys, tempfile, threading
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_teams_')
import live

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

class Icons:
    buffs = {}
    def buff(self, *a, **k): return None

m = live.Meter.__new__(live.Meter)
m.party = None; m.icon_read = None; m.icons = Icons(); m.icons_state = 'ready'
m.screen_settled = lambda kind, now: None
m.load_icons = lambda: None
m.card_shift = lambda ic, im, a, b, u, ux, H: (0, 0)
screen = {}                                  # card (pill) -> three names the stubbed reader sees (None: unreadable)
gate = threading.Event(); gate.set()
def team_tile(ic, im, t, b, u, ux, H, shift, memo=None):
    gate.wait(5)
    card = next(i for i, c in enumerate(live.Meter.PARTY_CARDS) if abs(t - c[0]) < 0.001 or abs(t - c[0] - live.Meter.TEAM_PITCH) < 0.001
                or abs(t - c[0] - 2 * live.Meter.TEAM_PITCH) < 0.001)
    i = round((t - live.Meter.PARTY_CARDS[card][0]) / live.Meter.TEAM_PITCH)
    name = im[card][i]
    return (name, 0.9, 0.3, 'k') if name else ('Someone', 0.4, 0.01, 'k')
m.team_tile = team_tile
m.lead_tile = lambda *a, **k: None               # (the lead's portrait: tests/party_lead_portrait.py)

def member(slot, name, lead, me=False):
    return {'slot': slot, 'name': name, 'self': me, 'ai': False, 'pill': slot, 'aniimo': lead, 'cp': 3000}
def read(members, now, pics):
    m.set_party(members, now)
    m.read_party_icons({k: list(v) for k, v in pics.items()}, [(x['slot'], x['pill']) for x in members], 2000, 0, 1125, now)
    m.icon_read.join(10)
lineup = lambda lead1='Leadb': [member(0, 'Alpha', 'Leada', True), member(1, 'Bravo', lead1), member(2, 'Charlie', 'Leadc')]
pics = {0: ['Ant', 'Bee', 'Cat'], 1: ['Dog', 'Eel', 'Fox'], 2: ['Gnu', 'Hen', 'Ibis']}

read(lineup(), 100, pics)
check('first read', m.party['teams'][:3], [['Ant', 'Bee', 'Cat'], ['Dog', 'Eel', 'Fox'], ['Gnu', 'Hen', 'Ibis']])
check('yours', m.party['team'], ['Ant', 'Bee', 'Cat'])

read(lineup(), 103, {**pics, 1: ['Dog', None, 'Fox']})          # a tile unreadable this time keeps its last read
check('unreadable keeps the last', m.party['teams'][1], ['Dog', 'Eel', 'Fox'])

read(lineup(), 106, {**pics, 0: ['Ant', 'Jay', 'Cat']})          # you swap one Aniimo: the next read shows it
check('a swap shows at once', m.party['team'], ['Ant', 'Jay', 'Cat'])

m.set_party(lineup('Leadz'), 120)                                # Bravo changes lead: their old team goes
check('a new lead: old team gone before any read', m.party['teams'][1], None)
check('...the others stay', m.party['teams'][2], ['Gnu', 'Hen', 'Ibis'])
read(lineup('Leadz'), 122, {**pics, 1: ['Kiwi', None, 'Lynx']})
check('a new lead: only what was read', m.party['teams'][1], ['Kiwi', None, 'Lynx'])

m.set_party([member(0, 'Alpha', 'Leada', True), member(2, 'Charlie', 'Leadc')], 124)   # Bravo's pill misread once
m.set_party([member(0, 'Alpha', 'Leada', True), member(2, 'Charlie', 'Leadc')], 126)   # ...twice: Bravo's gone
check('the slot list changed: the others keep their teams', m.party['teams'][2], ['Gnu', 'Hen', 'Ibis'])
m.set_party(lineup('Leadz'), 128)
check('a card back with the same player and lead', (m.party['teams'][0], m.party['teams'][1]), (['Ant', 'Bee', 'Cat'], None))

# a read running while card 2 changes hands, and a frame coming in meanwhile: read next, on the new player's card
gate.clear()
m.set_party(lineup('Leadz'), 130)
m.read_party_icons({k: list(v) for k, v in pics.items()}, [(0, 0), (1, 1), (2, 2)], 2000, 0, 1125, 130)
new = [member(0, 'Alpha', 'Leada', True), member(1, 'Bravo', 'Leadz'), member(2, 'Delta', 'Leadd')]
m.set_party(new, 132)
m.read_party_icons({**{k: list(v) for k, v in pics.items()}, 2: ['Mole', 'Newt', None]}, [(0, 0), (1, 1), (2, 2)], 2000, 0, 1125, 132)
gate.set(); m.icon_read.join(10)
check('a frame read after the running one', m.icon_next, None)
check('the new player: only their own card read', m.party['teams'][2], ['Mole', 'Newt', None])

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('party team reads: ok')
