"""Goldens for results.py's text parsers and core.py's Teammates / party_pill / screen signatures (aLogs B16), on
made-up OCR text and the synthetic frames.

    python -I tools/gen_results_goldens.py [reference/aLogs/bossbar]
"""
import copy, json, os, random, sys, tempfile
here = os.path.dirname(os.path.abspath(__file__))
src = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, "..", "reference", "aLogs", "bossbar"))
data = tempfile.mkdtemp(); os.environ["ALOGS_DATA"] = data
sys.path.insert(0, src)
import logging; logging.disable(logging.CRITICAL)
from PIL import Image
import core, results as RS
OUT = os.path.join(here, "..", "crates", "alogs-meter", "tests", "data")
rnd = random.Random(8)

def garble(s, p=0.15):
    out = []
    for c in s:
        r = rnd.random()
        if r < p / 3: continue
        if r < 2 * p / 3: out.append(rnd.choice("aeinrst 0O1l")); continue
        out.append(c)
        if r > 1 - p / 3: out.append(" ")
    return "".join(out)

g = {}
g["canon"] = [[t, RS.canon(t, RS.RESULT_STATS)] for t in [garble(rnd.choice(RS.RESULT_STATS), rnd.choice([0, 0.2, 0.5])) for _ in range(60)] + ["", "xyz", "Tot al Da nn age"]]
g["first_num"] = [[t, RS.first_num(t)] for t in ["60 60 60", "6 36 36", "O7 07", "", None, "a1b22c22", "3 4 5", "12 12 7 7"]]
sims = ["Battle time 00:08:22 Earth Training - Hard", "Battle time 00:08:22 Training of Water - Hard", "0:12:59 Trial of Fire - Challenge",
        "Battle time 01:61:00 Earth Training - Hard", "00:05:05 Lightning Training-Hardk", "00:05:05 Lightning Training -Hardkx",
        "00:05:05 Battle Training - Easy", "nothing", None, "00.03.04 Crucible of Dark .. Trial", "12:34:56 Dark  Trial - Common", "00:01:02 Ab Training - Easy"]
g["parse_sim"] = [[t, RS.parse_sim(t)] for t in sims]
names = ["Irisalis", "Ignitis", "Leafy", "Sparkelf"]
teams = []
for _ in range(12):
    t = {}
    for i in range(5):
        if rnd.random() < 0.8:
            t[f"c{i}w"] = f"Lv. {rnd.randint(1, 100)} {garble(rnd.choice(names + ['Repair Matrix', 'Nick Name']), 0.2)}"
        if rnd.random() < 0.5: t[f"c{i}b"] = f"Lv{rnd.randint(1, 120)} {rnd.choice(names)}"
    t["share"] = rnd.choice(["45% / 30 0/0 / 12%", "45 / 30 / 12", "x", "100% / 101% / 5%"])
    t["coins"] = rnd.choice(["8,550", "12345678", "", "8 550"]); t["coinsL"] = rnd.choice(["", "9,000"])
    t["gizmos"] = rnd.choice(["n: / 6 / 6 / 2 / 2 / 4 / 5", "n: 7 2 3", "", "n: 71 24 11"]); t["gizmosL"] = rnd.choice(["", "n: 1 2 3 4 5 6 7 8"])
    species = [rnd.choice([None, None, "Irisalis", "Leafy"]) for _ in range(5)] if rnd.random() < 0.5 else ()
    teams.append([t, list(species), RS.parse_sim_team(t, names, species)])
g["sim_team"] = teams
bds = ["Average DPS / BREAK Value / Damage Taken / Heal Amount / 12345 / 678 / 90 / 1",
       "Average DPS / BREAK Value / Damage Taken / Heal Amount / 12345 / 678 / 90 / Average DPS / BREAK Value / Damage Taken / Heal Amount / 1 / 2 / 3 / 4",
       "Average DPS / BREAK Value 2058717 / 0 Damage Taken 21398 / Heal Amount / Average DPS / BREAK Value / Damage Taken / Heal Amount / 5 / 6 / 7 / 8 / O Damage Taken / O Heal Amount / 28605 / 7737",
       "Average DPS / o / 1,234", "", None, "Average DPS / nothing", "junk Average DPS / BREAK / 5 / 6 / 7"]
g["battle_data"] = [[t, RS.parse_battle_data(t)] for t in bds]
seqs = []
for _ in range(10):
    votes = {}; out = []
    for _ in range(12):
        k = rnd.choice(["a", "b"]); v = rnd.choice([None, "", "x", "y", 3, 4])
        out.append([k, v, RS.vote(votes, k, v), RS.confirmed(votes, "a"), RS.confirmed(votes, "b"), sorted(RS.agreed_slots({f"n{i}": votes.get(k, {}) for i in range(4)}, lambda i: f"n{i}"))])
    seqs.append(out)
g["votes"] = seqs
with open(os.path.join(data, "teammates.json"), "w", encoding="utf-8") as f:
    json.dump({"AFKArashi": 5, "Kyhra": 3, "MinMax": 2, "Flora": 9}, f)
core.TEAM = RS.TEAM = core.Teammates(os.path.join(data, "teammates.json"))
g["teammates"] = {"seen": core.TEAM.seen, "fold": [[n, core.Teammates.fold(n)] for n in ["Élise", "AFK0r1", "|ink", "Straße", "ＡＢＣ"]],
                  "steady": [[n, core.TEAM.steady(n)] for n in ["AkArashi", "AFKAFashi", "Kyhru", "Kyh", "MinMax", "Flora", "Flor4", ""]]}
g["pills"] = [[p, i, core.party_pill(p, i)] for i, p in enumerate(["2P @ Flora O", "1PMinMax", ".4pR", "3P / Mira Vosk |", "@ iA", "IP Kyhra", "lP x", "2P 김철수"])]
slots = []
for _ in range(25):
    party = [{"slot": i, "name": rnd.choice(["Flora", "Kyhra", "AFKArashi", "MinMax", "0", ""]), "ai": rnd.random() < 0.3, "self": i == 1 and rnd.random() < 0.6}
             for i in range(rnd.randint(0, 4))]
    players = [{"slot": i, "name": garble(rnd.choice(["Flora", "Kyhra", "AFKArashi", "MinMax", "Mario", "0", "Q"]), 0.15)} for i in range(rnd.randint(1, 4))]
    sure = {i for i in range(4) if rnd.random() < 0.2}; agreed = {i for i in range(4) if rnd.random() < 0.3}
    you = rnd.choice([None, "Kyhra", "MinMax"])
    a, p = copy.deepcopy(players), copy.deepcopy(party)
    RS.party_slots(a, p, sure, you, agreed)
    b = copy.deepcopy(a); RS.mark_you(b, p, rnd.choice([None, "Kyhra"]))
    slots.append({"players": players, "party": party, "sure": sorted(sure), "agreed": sorted(agreed), "you": you,
                  "after": a, "marked": b, "known": None,
                  "own": RS.own_name(you, [x["name"] for x in players], party)})
g["slots"] = slots
frames = sorted(os.listdir(os.path.join(here, "..", "crates", "alogs-reader", "tests", "data", "frames")))[:4]
sig = []
for i, f in enumerate(frames):
    im = Image.open(os.path.join(here, "..", "crates", "alogs-reader", "tests", "data", "frames", f)).convert("RGB")
    other = Image.open(os.path.join(here, "..", "crates", "alogs-reader", "tests", "data", "frames", frames[(i + 1) % len(frames)])).convert("RGB").resize(im.size)
    a, b = core.screen_sig(im), core.screen_sig(other)
    sig.append({"file": f, "other": frames[(i + 1) % len(frames)], "sig": list(a), "top": list(core.top_sig_direct(im, im.height * 0.42)),
                "changed": core.screen_changed(a, b), "diff": RS.screen_difference(im, other)})
g["sigs"] = sig
with open(os.path.join(OUT, "results_goldens.json"), "w", encoding="utf-8", newline="\n") as fh: json.dump(g, fh, ensure_ascii=False)
print("ok", len(slots), "slot cases")
