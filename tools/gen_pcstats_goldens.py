"""Goldens for pcstats.alogs_groups (aLogs B16) on made-up process tables (no real process names but aLogs' own).

    python -I tools/gen_pcstats_goldens.py [reference/aLogs/bossbar]
"""
import json, os, random, sys
here = os.path.dirname(os.path.abspath(__file__))
src = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, "..", "reference", "aLogs", "bossbar"))
sys.path.insert(0, src)
import pcstats as P
rnd = random.Random(5)
NAMES = ["alogs-app.exe", "msedgewebview2.exe", "alogs-meter.exe", "ocr-helper.exe", "sanctumhelper.exe",
         "alogs-updating.exe", "alogs-installing.exe", "game.exe", "other.exe"]
cases = []
for _ in range(300):
    n = rnd.randint(3, 25)
    pids = rnd.sample(range(1, 200), n)
    table = {p: (rnd.choice([0] + pids), rnd.choice(NAMES)) for p in pids}
    starts = {p: (None if rnd.random() < 0.1 else rnd.randint(0, 50)) for p in pids}
    app, meter, sanctum = (rnd.choice(pids + [0, 999]) for _ in range(3))
    helpers = rnd.sample(pids + [500], rnd.randint(0, 3))
    g = P.alogs_groups(table, app, meter, sanctum, helpers, lambda q: starts.get(q))
    cases.append({"table": {str(k): list(v) for k, v in table.items()}, "starts": {str(k): v for k, v in starts.items()},
                  "app": app, "meter": meter, "sanctum": sanctum, "helpers": helpers,
                  "groups": {k: sorted(v) for k, v in g.items()}})
with open(os.path.join(here, "..", "crates", "alogs-pcstats", "tests", "data", "groups_goldens.json"), "w") as f:
    json.dump(cases, f)
print("pcstats goldens:", len(cases))
