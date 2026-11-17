"""Goldens for eggheist.py (aLogs B16): the rank ladder, titles, points, gains and seasons, on made-up OCR text.

    python -I tools/gen_egg_goldens.py [reference/aLogs/bossbar]
"""
import datetime, json, os, random, sys
here = os.path.dirname(os.path.abspath(__file__))
src = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, "..", "reference", "aLogs", "bossbar"))
sys.path.insert(0, src)
import eggheist as E
OUT = os.path.join(here, "..", "crates", "alogs-meter", "tests", "data")
rnd = random.Random(7)

def garble(s):
    return "".join(c if rnd.random() > 0.12 else rnd.choice("aeiou01lb ") for c in s)

titles = [n for n, *_ in E.RANKS] + [r[2] for r in E.LADDER] + ["", "Rank Info", "ProEggSnatcber II", "Pro Egg Snatcber", "Egg Pro"]
titles += [garble(t) for t in titles for _ in range(3)]
points = [None, 0, 1, 89, 90, 1277, 2370, 5000, 99999]
g = {"ladder": E.LADDER,
     "tier_of": [[t, E.tier_of(t)] for t in titles],
     "standing": [[t, p, s, E.standing(t, p, s)] for t in titles[:40] for p in points for s in (None, 3)],
     "gain_possible": [[gn, t, E.gain_possible(gn, t)] for gn in (None, -18, 0, 90, 91, 270, 271, 400) for t in (None, 0, 1, 3, 4, 7, 8)],
     "parse_points": [[t, list(E.parse_points(t))] for t in [None, "", "Rank Points 1277/2370", "1,277 / 2,37O", "l0/20", "oo/9", "12/", "Rank Points 4O0/ 5oo"]],
     "parse_gain": [[t, E.parse_gain(t)] for t in [None, "", "+18", "-18", "−18", "– 7", "+ 2O4", "18", "+12345", "x+3y"]],
     "parse_season": [[b, h, E.parse_season(b, h)] for b in [None, "", "S1", "5 2", "$3", "Season", "S12x", "XS1"]
                      for h in ["", "09/15/2026 - 11/15/2026", "1O/01/2026 -", "09/15/2026"]],
     "current_season": [[d, E.current_season(datetime.date.fromisoformat(d))] for d in ["2026-01-01", "2026-09-14", "2026-09-15", "2027-03-01"]]}
with open(os.path.join(OUT, "egg_goldens.json"), "w", encoding="utf-8") as f:
    json.dump(g, f, ensure_ascii=False)
print("egg goldens:", {k: len(v) for k, v in g.items()})
