"""Writes crates/alogs-gamedata/tests/data/hpcalc_goldens.json from the reference hpcalc.py (aLogs B16).

    python -I tools/gen_hpcalc_goldens.py [reference/aLogs/bossbar]

Numbers only: formula ids, made-up args, levels and the reference's outputs (floats as repr, so they round-trip).
"""
import json, os, sys

here = os.path.dirname(os.path.abspath(__file__))
src = sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, "..", "reference", "aLogs", "bossbar")
sys.path.insert(0, os.path.abspath(src))
import hpcalc  # noqa: E402

LEVELS = list(range(-1, 83))
ARGS4 = [[8.8, 1.176, 1.2, 0], [8.8, 1.176, 1.2, 1], [3.5, 0.9, 1.05, 1], [0.001, 0.5, 0.5, 0]]
ARGS2 = [[12.0, 0], [12.0, 1], [0.0001, 1]]
rate_cases = [{"1": 3, "2": 8.85}, {"1": 3016, "2": 1.1, "3": 2.4, "4": 3.9}, {"1": 3016, "2": 2.0, "3": 1.0, "4": 3.0}]
rate_cases += [dict({"1": f}, **{str(i + 2): v for i, v in enumerate(a)}) for f in (3101, 3133, 31331) for a in ARGS4]
rate_cases += [dict({"1": f}, **{str(i + 2): v for i, v in enumerate(a)}) for f in (3041, 3036, 3031, 3046, 3111, 3123) for a in ARGS2]
rate_cases += [{"1": 3101, "2": 8.8, "3": "x", "4": None}, {"1": 9999, "2": 1}, {"2": 1}, {"1": 3111, "2": True, "3": 1}]

out = {"rate": [], "total": [], "max_hp": [], "break_points": []}
for e in rate_cases:
    out["rate"].append({"entry": e, "values": [[lv, hpcalc.rate(e, lv)] for lv in LEVELS]})
out["rate"].append({"entry": [1, 2], "values": [[10, hpcalc.rate([1, 2], 10)]]})
for species in (0, 95, 110.5, 300):
    for ind in (0, 31):
        out["total"].append({"species": species, "ind": ind, "values": [[lv, hpcalc.total(species, lv, ind)] for lv in range(1, 91)]})
for e in rate_cases[:12]:
    for species in (95, 180):
        out["max_hp"].append({"species": species, "entry": e, "values": [[lv, hpcalc.max_hp(species, e, lv)] for lv in range(1, 81)]})
BP = [(3, [7.5]), (3109, [1.2, 1.5, 0]), (3109, [1.2, 1.5, 1]), (31091, [1.0, 2.0, 1]), (31092, [1.0, 2.0, 0]),
      (31092, [1.0, 2.0, 1]), (3050, [40, 0]), (3050, [40, 1]), (3051, [30, 1]), (3052, [35, 1.1, 0.9]), (3052, [80, 1, 1]),
      (3109, [2]), (4242, [1, 1, 1])]
for f, a in BP:
    out["break_points"].append({"formula": f, "args": a, "values": [[lv, hpcalc.break_points(f, a, lv)] for lv in LEVELS]})

dst = os.path.join(here, "..", "crates", "alogs-gamedata", "tests", "data", "hpcalc_goldens.json")
with open(dst, "w", encoding="utf-8", newline="\n") as f:
    json.dump(out, f, separators=(",", ":"))
print("wrote", os.path.normpath(dst), os.path.getsize(dst), "bytes")
