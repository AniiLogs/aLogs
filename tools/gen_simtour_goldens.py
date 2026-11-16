"""Goldens for the Gizmo tour's text helpers (aLogs B16 leaderboards.py: sim_names, sim_stitch, sim_count) on made-up names.

    python -I tools/gen_simtour_goldens.py [reference/aLogs/bossbar]
"""
import json, os, random, sys, types
here = os.path.dirname(os.path.abspath(__file__))
src = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, "..", "reference", "aLogs", "bossbar"))
sys.path.insert(0, src)
import logging; logging.disable(logging.CRITICAL)
import leaderboards as L
rnd = random.Random(9)
GIZ = {"Predictive Dice": ("1", "Core"), "Fervor Mask": ("1", "Core"), "Spark Coil": ("2", "Normal"), "Tidal Lens": ("2", "Uncommon"),
       "Ember Gear": ("3", "Normal"), "Frost Pin": ("3", "Uncommon"), "Gale Feather": ("4", "Normal"), "Stone Ward": ("4", "Normal")}
me = types.SimpleNamespace(icons=types.SimpleNamespace(sim_gizmos=GIZ))
def garble(s): return "".join(c if rnd.random() > 0.08 else rnd.choice("aeiou l1") for c in s)
lines = []
for _ in range(200):
    parts = []
    for _ in range(rnd.randint(1, 6)):
        n = rnd.choice(list(GIZ)); tag = rnd.choice(["", " Normal", " Core", ": Final", " a", " α", ": +5% damage"])
        parts.append(garble(n + tag) if rnd.random() < 0.5 else n + tag)
    lines.append(" / ".join(parts))
names = [[t, sid, L.LeaderboardsMixin.sim_names(me, t, sid)] for t in lines for sid in (None, "2")]
stitch = []
for _ in range(200):
    rows = [rnd.choice(list(GIZ)) + rnd.choice(["", ": Final", " α"]) for _ in range(rnd.randint(0, 6))]
    k = rnd.randint(0, len(rows)); view = rows[len(rows) - k:] + [rnd.choice(list(GIZ)) for _ in range(rnd.randint(0, 4))]
    if rnd.random() < 0.3: view = [rnd.choice(list(GIZ)) for _ in range(rnd.randint(0, 4))]
    stitch.append([rows, view, L.LeaderboardsMixin.sim_stitch(rows, view), L.LeaderboardsMixin.sim_count(rows)])
with open(os.path.join(here, "..", "crates", "alogs-meter", "tests", "data", "simtour_goldens.json"), "w", encoding="utf-8") as f:
    json.dump({"gizmos": {k: list(v) for k, v in GIZ.items()}, "names": names, "stitch": stitch}, f, ensure_ascii=False)
print("sim tour goldens:", len(names), len(stitch))
