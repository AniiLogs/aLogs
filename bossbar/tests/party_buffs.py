"""The Party screen's Season Buffs and Special Offering are read where the panel is, and kept between reads.

    python tests/party_buffs.py

Meter.read_party_job / set_party (7 Nov: 10 of 39 runs sent a Special Offering, though 34 of their Party screens show
one):
- when the whole screen is drawn higher (every card ~40 px up at 4K), the buffs panel is found ~1.9 times as far up;
  a few px of one card's fit don't move it;
- a panel a little lower than measured (the cards where they should be) is found around its place;
- a read that misses the offering (or one Season Buff) keeps the last one read;
- the buffs stay through a party rebuilt after a misread pill while 1P is the same player, and go with a new 1P.
Made-up buffs; the picture readers are stubbed (no game files, no screenshots). Exit code 1 on any failure.
"""
import os, sys, tempfile
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_buffs_')
import live
from live import Meter

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

W, H = 3840, 2160
u, ux = W, 0
class Icons:
    buffs = {'1': {'name': 'Joint Guard', 'kind': 'season', 'icon': 'guard', 'level': 7},
             '2': {'name': 'Team Strike', 'kind': 'season', 'icon': 'strike', 'level': 7},
             '3': {'name': 'Hourglass', 'kind': 'offering', 'icon': 'glass'}}
    def buffs_named(self, icon, kind, level=None):
        return [k for k, b in self.buffs.items() if b['icon'] == icon and b['kind'] == kind and (level is None or b.get('level') == level)]
    def buff(self, im, box, kind, text_below=True):
        """What a stubbed screen shows at box: a match only within 3 px of where its panel really is."""
        y = box[1] - im['panel']
        squares = [(Meter.OFFERING, im['offering'])] if kind == 'offering' else list(zip(Meter.SEASON_BUFFS, im['season']))
        for sq, icon in squares:
            if abs(box[0] - (ux + u * sq[0])) < 3 and abs(y - H * sq[1]) < 3:
                return (icon, 0.9, 0.4) if icon else ('none', 0.3, 0.05)
        return ('none', 0.1, 0.01)
    def digit(self, im, box): return ('7', 0.1, 0.3)

m = Meter.__new__(Meter)
m.icons = Icons(); m.screen_settled = lambda kind, now: None
m.team_tile = lambda *a, **k: None
def screen(cards_dy, panel_dy, offering='glass', season=('guard', 'strike'), wobble=0):
    m.card_shift = lambda ic, im, a, b, u_, ux_, H_: (0, cards_dy + (wobble if a == Meter.PARTY_CARDS[0][0] else 0))
    return {'panel': panel_dy, 'offering': offering, 'season': list(season)}
def member(slot, name): return {'slot': slot, 'name': name, 'self': slot == 0, 'ai': False, 'pill': slot, 'aniimo': 'Lead'}
def read(im, members=None):
    members = members or m.party['members']
    m.read_party_job(im, [(x['slot'], x['pill']) for x in members], u, ux, H, {x['slot']: m.who(x) for x in members})
    return [b['name'] for b in m.party['buffs']]
full = [member(0, 'Alpha'), member(1, 'Bravo'), member(2, 'Charlie')]

m.party = None; m.set_party(full, 100)
check('where measured', read(screen(0, 0)), ['Joint Guard', 'Team Strike', 'Hourglass'])
check('...with levels', [b.get('level') for b in m.party['buffs']], [7, 7, None])

m.party = None; m.set_party(full, 200)
check('screen drawn 40 px higher: panel ~76 px up', read(screen(-40, -76)), ['Joint Guard', 'Team Strike', 'Hourglass'])
m.party = None; m.set_party(full, 300)
check('one card wandering a few px: no shift', read(screen(-2, 0, wobble=-5)), ['Joint Guard', 'Team Strike', 'Hourglass'])
m.party = None; m.set_party(full, 400)
check('panel 1% lower, cards in place', read(screen(0, round(H * 0.01))), ['Joint Guard', 'Team Strike', 'Hourglass'])

m.party = None; m.set_party(full, 500)
read(screen(0, 0))
check('a read missing the offering keeps it', read(screen(0, 0, offering=None)), ['Joint Guard', 'Team Strike', 'Hourglass'])
check('...and one missing a Season Buff', read(screen(0, 0, season=('guard', None))), ['Joint Guard', 'Team Strike', 'Hourglass'])

m.set_party([member(0, 'Alpha'), member(2, 'Charlie')], 503)                  # Bravo's pill misread twice
m.set_party([member(0, 'Alpha'), member(2, 'Charlie')], 506)
check('rebuilt, same 1P: buffs kept', [b['name'] for b in m.party.get('buffs', [])], ['Joint Guard', 'Team Strike', 'Hourglass'])
m.set_party([member(0, 'Delta'), member(1, 'Bravo'), member(2, 'Charlie')], 509)
check('a new 1P: their buffs, not the last', m.party.get('buffs'), None)

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('party buffs: ok')
