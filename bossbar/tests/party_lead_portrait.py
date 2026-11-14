"""The Party screen's lead Aniimo goes by its head portrait, not by the name on the card (14 Nov: since the affinity
update players can rename their Aniimo, so that name says nothing about the species).

    python tests/party_lead_portrait.py

- Meter.lead_tile reads every card's lead on the four saved Party screens (test_shots/party_screen*.webp/png: 16
  leads, 1P..4P in PARTY_CARDS order) with the game's own portraits, and each one is the species shown and sure
  (tile_sure), placed as the game lays the card out (LEAD_OFFSET from the first team tile, LEAD_PORTRAIT square)
  with the lead's own fade (icons.LEAD_FADE).
- party_teams puts a sure lead on its card as `aniimo`; a later read that isn't sure keeps it; another species on the
  same card starts its team over; a lead read the first time keeps the team read so far.
- party_screen keeps the name on the card as `nickname` only, never as the species, and a renamed lead (the same CP)
  keeps the species read from its portrait.
- submit._lineup sends the portrait's species as `aniimo` and the card's name as `nickname`.
Made-up players; the game's icons are read from this PC's install (skipped when they can't be). Exit code 1 on any
failure.
"""
import os, sys, tempfile, time, logging
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
logging.disable(logging.CRITICAL)
from PIL import Image
import live as L
import submit

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

# -- the party bookkeeping (no pictures) ------------------------------------------------------------------------------
m = L.Meter.__new__(L.Meter); m.party = None; m.egg_party = False
m.screen_settled = lambda kind, now: None
member = lambda slot, name, **k: {'slot': slot, 'name': name, 'self': slot == 0, 'ai': False, 'pill': slot, 'cp': 3000 + slot, **k}
m.set_party([member(0, 'Alpha', nickname='Sparky'), member(1, 'Bravo', nickname='Glacy')], 100)
who = lambda: {x['slot']: m.who(x) for x in m.party['members']}
m.party_teams({0: ['Ant', 'Bee', 'Cat'], 1: ['Dog', None, 'Fox']}, who(), {0: 'Hypnyto', 1: None})
lead = lambda slot: next(x for x in m.party['members'] if x['slot'] == slot).get('aniimo')
check('a sure lead goes on its card', lead(0), 'Hypnyto')
check('an unsure one stays unread (not the name on the card)', lead(1), None)
m.party_teams({0: ['Ant', None, 'Cat'], 1: ['Dog', 'Eel', 'Fox']}, who(), {0: None, 1: 'Glacy'})
check('a read that isn\'t sure keeps the lead', lead(0), 'Hypnyto')
check('...and its team', m.party['teams'][0], ['Ant', 'Bee', 'Cat'])
check('a lead read the first time keeps the team read so far', (lead(1), m.party['teams'][1]), ('Glacy', ['Dog', 'Eel', 'Fox']))
m.party_teams({1: ['Gnu', None, None]}, who(), {1: 'Buffby'})
check('another species: a new lead, its team starts over', (lead(1), m.party['teams'][1]), ('Buffby', ['Gnu', None, None]))
# a frame that waited while the read before it learnt the lead: still the same card
waited = {0: ('alpha', ''), 1: m.who(next(x for x in m.party['members'] if x['slot'] == 1))}
m.party_teams({0: ['Ant', 'Jay', None]}, waited, {0: None})
check('a frame from before the lead was known still counts', m.party['teams'][0], ['Ant', 'Jay', 'Cat'])

# party_screen: the card's name is a nickname; a renamed lead with the same CP keeps its species
class Ocr:
    reads = []
    def read(self, crops, **k): return {'again': ''} if 'again' in crops else self.reads.pop(0)
p = L.Meter.__new__(L.Meter); p.ocr = Ocr(); p.party = None; p.character_name = 'Alpha'; p.catalog = []
p.screen_settled = lambda *a: None; p.read_party_icons = lambda *a: None; p.cycle_boss = lambda b: None; p.stage_first = lambda r: r
p.party_names = lambda *a: {}
im = Image.new('RGB', (1280, 720), (40, 60, 90))
def screen(now, card):
    Ocr.reads = [{'n0': '1P Alpha', 'a0': card, 'boss': ''}]; p.party_seen = 0
    p.party_screen(im, now, 1280, 0, 720)
    return p.party['members'][0]
first = screen(100, 'Irisalis / Lv.60 CP:3578')
check('the name on the card is its nickname', (first.get('aniimo'), first.get('nickname')), (None, 'Irisalis'))
first['aniimo'] = 'Hypnyto'                                   # (as read_party_job's portrait read puts it)
renamed = screen(103, 'Pinky / Lv.60 CP:3578')
check('renamed, same CP: the species stays', (renamed.get('aniimo'), renamed.get('nickname')), ('Hypnyto', 'Pinky'))
again = screen(106, 'Pinky / Lv.60 CP:3570')
check('a CP misread under the same name: the same lead', again.get('aniimo'), 'Hypnyto')
new = screen(109, 'Buffby / Lv.60 CP:2950')
check('another name and CP: a new lead, unread', (new.get('aniimo'), new.get('nickname')), (None, 'Buffby'))

line = submit._lineup({'party': [member(0, 'Alpha', aniimo='Hypnyto', nickname='Pinky', aniimo_level=60),
                                 member(1, 'Bravo', nickname='Glacy')], 'teams': [['Ant', None, 'Cat'], None]})['lineup']
check('sent: the species from the portrait, the name as its nickname', (line[0].get('aniimo'), line[0].get('nickname')), ('Hypnyto', 'Pinky'))
check('sent: no species when the portrait didn\'t read', ('aniimo' in line[1], line[1].get('nickname')), (False, 'Glacy'))

# -- the 16 leads on the saved Party screens ----------------------------------------------------------------------------
TRUTH = {'party_screen.webp': ['Hypnyto', 'Buffby', 'Fenmane', 'Glacy'],
         'party_screen_minespine.webp': ['Irisalis', 'Panpanta', 'Irisal', 'Glacy'],
         'party_screen_scorchhowl.webp': ['Sherro', 'Shrubclaw', 'Grizbo', 'Glacy'],
         'party_screen_2p_real.png': ['Irisalis', 'Irisalis', 'Irisalis', 'Thornblade']}
r = L.Meter.__new__(L.Meter); r.icons = None; r.icons_state = None
class Args: player_log = None
r.args = Args(); r.load_icons()
for _ in range(900):
    if r.icons_state in ('ready', 'failed'): break
    time.sleep(0.1)
if r.icons_state != 'ready':
    print("SKIP the leads on the saved screens: the game's icons aren't readable on this PC")
else:
    ic = r.icons
    check('the lead fade is in the cache', 'lead' in ic.fades, True)
    for shot, want in TRUTH.items():
        im = Image.open(os.path.join(HERE, 'test_shots', shot)).convert('RGB')
        W, H = im.size; u = min(W, H * 16 / 9); ux = (W - u) / 2
        for card, species in enumerate(want):
            a, b = L.Meter.PARTY_CARDS[card][:2]
            got = r.lead_tile(ic, im, a, b, u, ux, H, r.card_shift(ic, im, a, b, u, ux, H))
            print(f'  {shot} card {card + 1}: {got[0] if got else None} {got[1]:.3f} ahead by {got[2]:.3f}' if got else f'  {shot} card {card + 1}: none')
            check(f'{shot} card {card + 1}', (got[0] if got else None, r.tile_sure(got)), (species, True))

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('party lead portrait: ok')
