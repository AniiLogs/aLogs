"""The Holo-Battle Sim's Victory screen comes in two heights: with a "Rewards Earned" row (a first clear,
test_shots/sim_victory.webp) and without one (3 Nov, test_shots/sim_victory_no_rewards.webp), where the right side
- coins, Data Collection pictures, gizmo row and counts - sits lower. Without the gizmo row read, aLogs gave control
back at once and never opened the Gizmo screen. This reads both shots the way sim_result does (one OCR call, then the
pictures with the game's icons) and checks the coins, counts and series row of each.
Usage: python tests/sim_victory_layouts.py"""
import os, sys, logging, time
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__))); sys.path.insert(0, HERE)
logging.disable(logging.CRITICAL)
from PIL import Image
import live as L
import reader as R

failed = False
def check(name, ok):
    global failed
    print(('PASS ' if ok else 'FAIL ') + name); failed |= not ok

m = L.Meter.__new__(L.Meter); m.icons = None; m.icons_state = None
class Args: player_log = None
m.args = Args()
m.load_icons()
for _ in range(300):
    if m.icons_state in ('ready', 'failed'): break
    time.sleep(0.1)
if m.icons_state != 'ready':
    print("SKIP the game's icons aren't readable on this PC"); sys.exit(0)

ocr = R.Ocr()
CASES = (('sim_victory.webp', 8550, [6, 6, 2, 2, 4, 5], False),
         ('sim_victory_no_rewards.webp', 6250, [9, 6, 4, 1, 4, 2, 1], True))
try:
    for shot, coins, counts, low in CASES:
        im = Image.open(os.path.join(HERE, 'test_shots', shot)).convert('RGB')
        W, H = im.size; u = min(W, H * 16 / 9); ux = (W - u) / 2
        team = L.parse_sim_team(ocr.read(L.sim_team_crops(im, u, ux, H)))
        check(f'{shot}: coins {team["coins"]}', team['coins'] == coins)
        check(f'{shot}: gizmo counts {team["gizmoCounts"]}', team['gizmoCounts'] == counts)
        check(f'{shot}: read {"lower" if team["low"] else "at the usual height"}', team['low'] == low)
        pics = m.sim_pictures(im, u, ux, H, L.SIM_LOW if team['low'] else 0.0)
        row = pics.get('gizmos') or []
        check(f'{shot}: gizmo row {len(row)} series, one per count', len(row) == len(counts) and all(g.get('sid') for g in row))
finally:
    getattr(ocr, 'close', lambda: None)()
print('ALL PASS' if not failed else 'SOME FAILED'); sys.exit(1 if failed else 0)
