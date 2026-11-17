"""Goldens for submit.py's payload builders (aLogs B16) on made-up run summaries (no player's data).

    python -I tools/gen_payload_goldens.py [reference/aLogs/bossbar]

time.time is fixed and time.mktime is calendar.timegm, so both sides agree whatever the PC's time zone.
"""
import calendar, copy, json, os, random, sys, tempfile
here = os.path.dirname(os.path.abspath(__file__))
src = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, "..", "reference", "aLogs", "bossbar"))
data = tempfile.mkdtemp(); os.environ["ALOGS_DATA"] = data
sys.path.insert(0, src)
import logging; logging.disable(logging.CRITICAL)
import time
NOW = 1_800_000_000.0
time.time = lambda: NOW
time.mktime = calendar.timegm
import reader as R, submit as S
OUT = os.path.join(here, "..", "crates", "alogs-meter", "tests", "data")
R.CYCLES_FILE[0] = os.path.join(OUT, "cycles.json")
rnd = random.Random(12)
NAMES = ["Kyhra", "AFKArashi", "MinMax", "Flora", "0", "Mira Vosk", "Zarvok", "Ashgrove"]

def garble(s):
    return "".join(c if rnd.random() > 0.15 else rnd.choice("aeiou01l") for c in s)

def summary():
    n = rnd.randint(1, 4)
    slots = rnd.sample(range(4), n)
    who = rnd.sample(NAMES, 4)
    party = [{"slot": i, "name": garble(who[i]) if rnd.random() < 0.7 else who[i], "ai": rnd.random() < 0.2, "self": i == 0,
              "aniimo": rnd.choice([None, "Irisalis", "Leafy", "Nick"]), "aniimo_level": rnd.choice([None, 60]), "cp": rnd.choice([None, 12345]),
              "nickname": rnd.choice([None, "Bob"]), **({"pill": rnd.choice([0, 1, 2])} if rnd.random() < 0.3 else {})}
             for i in range(rnd.randint(0, 4))]
    players = [{"slot": s, "name": rnd.choice([who[s], garble(who[s]), "", None]), "stat": rnd.choice([None, "Total Damage Dealt"]),
                "value": rnd.choice([None, 12345]), "ai": rnd.choice([None, True, False, "x"]), "self": s == 0 and rnd.random() < 0.5,
                **({"sure": True} if rnd.random() < 0.2 else {})} for s in slots]
    if rnd.random() < 0.2:
        for p in players: p.pop("slot", None)
    cards = [{"slot": s, "name": who[s], "avg_dps": rnd.randint(1000, 99999), "break": rnd.randint(0, 9999), "taken": rnd.choice([None, 5]),
              "heal": 0, "ai": False, "self": s == 1 and rnd.random() < 0.5} for s in rnd.sample(range(4), rnd.randint(0, 4))]
    s = {"result": {"players": players, "score": rnd.randint(1000, 40000), "rating_stars": rnd.randint(0, 40), "time_s": rnd.choice([None, 95, 180]),
                    "cleared": rnd.random() < 0.5, "new_best": rnd.random() < 0.3, "star_cap": rnd.choice([None, 40]),
                    "ended_by_leader": rnd.random() < 0.1, "screen_check": rnd.choice([None, {"worst": 3}])},
         "battle_data": {"players": cards, "team_avg_dps": rnd.randint(1000, 99999), "screen_check": rnd.choice([None, {"worst": 9}])},
         "meter": {"csv": rnd.choice(["20261101_120000_OmegaX.csv", None]), "dps": 1234.5, "damage": 999, "tracked_from_start": True,
                   "time_limit_s": rnd.choice([None, 240]),
                   **({"bosses": [{"id": 500102, "name": "Omega Two", "max_hp": 5, "damage": 5, "defeated": True, "seconds": 40.5},
                                  {"id": 500202, "name": None, "max_hp": 6, "damage": 2, "defeated": False, "seconds": None}]} if rnd.random() < 0.3 else {})},
         "party": party, "teams": [rnd.choice([None, [], ["Irisalis", "", "Leafy"]]) for _ in range(4)], "team": ["Sparkelf"],
         "uid": rnd.choice([None, "100000000001"]), "cycle": rnd.choice(["4004", None, "9999"]), "puppetId": rnd.choice(["500102", "500202", "1"]),
         "boss": "Omega Two", "level": 60, "started": rnd.choice(["2026-11-01 12:34:56", None, "bad"]), "instance": "i-1",
         "new_bests": rnd.choice([None, ["4004"]]), "buffs": rnd.choice([None, [{"id": 1, "name": "X", "level": None}]]),
         **({"server": "asia"} if rnd.random() < 0.3 else {})}
    return s

runs = []
for _ in range(40):
    s = summary()
    p = S.run_payload(copy.deepcopy(s), "100000000009")
    runs.append({"summary": s, "payload": p, "notice": S.sent_notice(p, rnd.choice([None, {"updated": True}])),
                 "checked": S.send_check(p, {"irisalis", "leafy", "sparkelf"}), "checked_none": S.send_check(p)})
scores = [[a, S.score_payload(*a)] for a in [("challenge_stats", "", 12345, 30, "1", "4004"), ("prep", "Omega Two", 9, 3, "1", "4004", "500202"),
                                              ("prep", "Omega " * 30, 9, 3, None, "4004")]]
sims = [[a, S.sim_payload(*a)] for a in [({"time_s": 502, "trial": "Earth Training", "element": "Earth", "difficulty": "Hard"}, "1", None),
                                           ({"time_s": 5, "trial": "T", "element": "E", "difficulty": "D", "setup": {"team": [{"name": "Nick"}]}}, None, 1_799_999_000)]]
eggs = [[a, S.egg_match_payload(*a)] for a in [({"loot": 40, "time_s": 300, "outcome": "success", "difficulty": "Hard", "defeats": 2, "mode": "Egg Heist"}, "S1", "1"),
                                                ({"outcome": "x", "difficulty": "Hard!!", "screen_check": {"w": 1}, "party": {"party": [{"slot": 0, "name": "A"}]}}, "S1", None, 1_799_000_000)]]
seasons = [[a, S.eggheist_payload(*a)] for a in [({"tier": 3, "subtier": 2, "name": "Gold " * 20, "points": 1319, "stars": 4}, "S2", "1", {"nextRankAt": 2370, "gain": None})]]
notices = [[p, S.sent_notice(p)] for p in [x[1] for x in scores] + [x[1] for x in sims] + [x[1] for x in eggs] + [x[1] for x in seasons] + [{"kind": "interlink_week"}]]
with open(os.path.join(OUT, "payload_goldens.json"), "w", encoding="utf-8", newline="\n") as fh:
    json.dump({"now": NOW, "runs": runs, "scores": scores, "sims": sims, "eggs": eggs, "seasons": seasons, "notices": notices}, fh, ensure_ascii=False)
print(len(runs), "runs")
