"""Holo-Battle Sim's Victory team goes by each card's art, not the name on it (14 Nov: since the affinity update players
can rename their Aniimo, so the card's text can be a nickname).

    python tests/sim_victory_card_art.py

- Icons.card names every Aniimo on the two saved Victory screens (test_shots/sim_victory*.webp) from the game's own
  card art (UI_Img_ResearchBook_Enter_*), placed as UI_Pb_Tower_Settlement lays the cards out (results.SIM_CARD_ART),
  each sure (SIM_CARD_SCORE / SIM_CARD_MARGIN); a run's equipment card in a team slot (Repair Matrix) is none of them.
- sim_badge_counts reads each Holo-gizmo count badge on its own against the game's digits: 6 6 2 2 4 5 and
  9 6 4 1 4 2 1 on the two shots, the lone 1s included (one OCR line of all the badges dropped them), nothing at the
  other layout's height.
- parse_sim_team takes the art's species as the name, keeps a card's other text as its nickname (not a misread of the
  species' own name), keeps a card the art didn't name when its text is an Aniimo's name, and drops one whose text
  isn't (the equipment card) once any art was read.
The game's icons are read from this PC's install (skipped when they can't be). Exit code 1 on any failure.
"""
import os, sys, logging, time
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__))); sys.path.insert(0, HERE)
logging.disable(logging.CRITICAL)
from PIL import Image
import results

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

# -- the team from the art and the text (no pictures) ---------------------------------------------------------------
t = {'c0w': 'Lv. 70 Sparkles', 'c1w': 'Lv. 60 Hexxln', 'c2w': 'Lv. 60 Repair Matrix', 'c3w': 'Lv. 60 Turbo', 'c4w': 'Lv. 60 Leafy'}
names = ['Hexxin', 'Irisalis', 'Leafy', 'Turbo']
team = results.parse_sim_team(t, names, ['Irisalis', 'Hexxin', None, None, 'Leafy'])['team']
check('the art names the MVP, its text its nickname', team[0], {'name': 'Irisalis', 'level': 70, 'mvp': True, 'nickname': 'Sparkles'})
check("a misread of the species' own name isn't a nickname", team[1], {'name': 'Hexxin', 'level': 60, 'mvp': False})
check("a card the art didn't name: its text when that's an Aniimo, the equipment card dropped",
      [m['name'] for m in team], ['Irisalis', 'Hexxin', 'Turbo', 'Leafy'])
check('no art read at all: the text alone, as before',
      [m['name'] for m in results.parse_sim_team(t, names, [None] * 5)['team']], ['Sparkles', 'Hexxin', 'Repair Matrix', 'Turbo', 'Leafy'])

# -- the art on the saved screens -----------------------------------------------------------------------------------
import live as L
m = L.Meter.__new__(L.Meter); m.icons = None; m.icons_state = None
class Args: player_log = None
m.args = Args()
m.load_icons()
for _ in range(900):
    if m.icons_state in ('ready', 'failed'): break
    time.sleep(0.1)
if m.icons_state != 'ready' or not getattr(m.icons, 'cards', None):
    print("SKIP the card art: the game's icons aren't readable on this PC")
else:
    CASES = (('sim_victory.webp', ['Irisalis', 'Hexxin', 'Somniwing', 'Leafy', 'Turbo']),
             ('sim_victory_no_rewards.webp', ['Pawney', 'Inferlupa', None, 'Turbo', 'Hexxin']))   # slot 3: Repair Matrix
    for name, want in CASES:
        im = Image.open(os.path.join(HERE, 'test_shots', name)).convert('RGB')
        W, H = im.size; u = min(W, H * 16 / 9); ux = (W - u) / 2
        t0 = time.time()
        check(f'{name}: each card by its art', m.sim_species(im, u, ux, H), want)
        print(f'{name}: {time.time() - t0:.1f} s')
    reader = m.name_reader()
    if reader is None: print("SKIP the counts: the game's font isn't readable on this PC")
    else:
        for name, low, want in (('sim_victory.webp', False, [6, 6, 2, 2, 4, 5, None]),
                                ('sim_victory_no_rewards.webp', True, [9, 6, 4, 1, 4, 2, 1])):
            im = Image.open(os.path.join(HERE, 'test_shots', name)).convert('RGB')
            W, H = im.size; u = min(W, H * 16 / 9); ux = (W - u) / 2
            check(f'{name}: the counts by the game digits', results.sim_badge_counts(im, u, ux, H, reader, results.SIM_LOW if low else 0.0), want)
            check(f'{name}: nothing at the other height', results.sim_badge_counts(im, u, ux, H, reader, 0.0 if low else results.SIM_LOW), [None] * 7)

if fails:
    print('FAIL\n  ' + '\n  '.join(fails)); sys.exit(1)
print('PASS')
