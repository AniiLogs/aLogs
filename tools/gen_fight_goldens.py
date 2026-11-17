"""Scripted fights through the reference fights.Fight (aLogs B16), with its state after every frame, as goldens.

    python -I tools/gen_fight_goldens.py [reference/aLogs/bossbar]

The frames are made up here (no captures): an Interlink one-boss run with counter misreads, unreadable frames and a
fake refill; a two-boss middle fight handing over at x1; a world boss whose bar slides in nearly empty; plus Timer
and BreakTracker sequences. time.time is patched so both sides see the same clock.
"""
import json, os, random, sys, tempfile
here = os.path.dirname(os.path.abspath(__file__))
src = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, "..", "reference", "aLogs", "bossbar"))
data = tempfile.mkdtemp()
os.environ["ALOGS_DATA"] = data
sys.path.insert(0, src)
import logging; logging.disable(logging.CRITICAL)
import time
CLOCK = [1_800_000_000.0]
time.time = lambda: CLOCK[0]
import fights as F
import reader as R
import devflags as D

OUT = os.path.join(here, "..", "crates", "alogs-meter", "tests", "data")
os.makedirs(OUT, exist_ok=True)
sched = os.path.join(OUT, "cycles.json")
with open(sched, "w", encoding="utf-8") as f:
    json.dump([{"cycle": "4004", "start": 1_799_000_000, "end": 1_801_000_000, "split": 91,
                "bossIds": [500101, 500102, 500103], "bossIds2": [None, 500202, None]}], f)
R.CYCLES_FILE[0] = sched
with open(os.path.join(src, "boss_break.json"), encoding="utf-8") as f: prop = next(iter(json.load(f)))
ROW = lambda pid, title: {"puppetId": str(pid), "table": "puppet_data", "bossTitle_en": title, "name_en": title,
                         "species_hp": "95", "prop_species_adj": "0", "propId": prop,
                         "fixed_hp_rate": '{"1": 3101, "2": 8.8, "3": 1.176, "4": 1.2, "5": 0}', "in_boss_rush": "yes"}
GRADES = R.load_grades()
rnd = random.Random(3)
RED, WHITE = (240, 110, 100), (250, 250, 250)


def state(f):
    return [f.seg, f.seg_fill, f.damage, f.elapsed, f.avg, f.recent, f.limit, f.hit, f.t0, f.refill_pending, len(f.phases),
            f.max_hp, f.boss_hp, f.done_hp, f.started_at_top, f.low_pending and list(f.low_pending), f.first_damage,
            f.seg_info and [f.seg_info["grade"], f.seg_info["size_hp"], f.seg_info["left_hp"]], f.order, f.column,
            [[p["damage"], p["defeated"], p["start_s"], p["end_s"], p.get("hit_s")] for p in f.phases]]


def run(name, fight_args, frames, slide=False):
    D.use(None)
    if slide:
        p = os.path.join(data, "dev.json")
        with open(p, "w") as fh: json.dump({"on": ["fight-bar-slide-in"]}, fh)
        D.use(p)
    CLOCK[0] = 1_800_000_000.0
    f = F.Fight(*fight_args)
    out = []
    for fr in frames:
        CLOCK[0] = fr["at"]
        f.update(fr["fill"], tuple(fr["rgb"]), (40, 30, 30), fr["xn"], fr["timer"], GRADES, 180, at=fr["at"])
        if fr.get("defeat"): f.mark_defeated("test")
        out.append(state(f))
    f.close()
    return {"name": name, "args": [fight_args[0], fight_args[1], fight_args[2], fight_args[3], fight_args[4],
                                   fight_args[6] if len(fight_args) > 6 else None, fight_args[7] if len(fight_args) > 7 else None],
            "slide": slide, "frames": frames, "states": out}


def interlink(t0, bars, top=21, misread=0.04, unreadable=0.03, fake_refill_at=None, step=0.1, clock=180):
    frames, t = [], t0
    for xn in range(top, top - bars, -1):
        n = rnd.randint(6, 25)
        for i in range(n):
            fill = max(0.0, 1.0 - (i + 1) / n + rnd.uniform(-0.01, 0.01))
            x = xn if rnd.random() > misread else rnd.choice([xn + 1, xn - 1, 11, 7, None])
            rgb = RED if rnd.random() > unreadable else WHITE
            timer = int(clock - (t - t0)) if rnd.random() < 0.3 else None
            if timer is not None and rnd.random() < 0.05: timer = rnd.randint(0, 999)
            frames.append({"at": round(t, 3), "fill": round(fill, 4), "rgb": list(rgb), "xn": x, "timer": timer})
            t += step
        if fake_refill_at == xn:
            for _ in range(3):
                frames.append({"at": round(t, 3), "fill": 0.9, "rgb": list(RED), "xn": None, "timer": None}); t += step
    return frames, t


cases = []
fr, _ = interlink(1_800_000_000.0, 21, fake_refill_at=15)
cases.append(run("interlink", ("Omega One", 60, ROW(500101, "Omega One"), 33_000_000, True), fr))
fr1, t = interlink(1_800_000_000.0, 21, misread=0.02)
fr2, _ = interlink(t, 6, misread=0.02, clock=180 - (t - 1_800_000_000.0))
fr2[-1]["defeat"] = True
cases.append(run("two_boss", ("Omega Two", 60, ROW(500102, "Omega Two"), 20_000_000, True, None, ROW(500202, "Omega Partner"), 1), fr1 + fr2))
world, t = [], 1_800_000_000.0
for i in range(40):                                       # slides in reading ~5%, then full, then drained by hits
    fill = 0.05 if i < 8 else max(0.0, 1.0 - max(0, i - 15) * 0.03)
    world.append({"at": round(t, 3), "fill": fill, "rgb": list(RED), "xn": None, "timer": None}); t += 0.25
cases.append(run("world_slide", ("Alpha Wild", 55, ROW(210001, "Alpha Wild"), 400_000, False), world, slide=True))
cases.append(run("world_plain", ("Alpha Wild", 55, ROW(210001, "Alpha Wild"), 400_000, False), world, slide=False))

timers = []
for _ in range(6):
    tm = F.Timer(180); seq, t = [], 0.0
    for i in range(80):
        r = rnd.choice([None, None, int(180 - t), int(180 - t), rnd.randint(0, 999), int(150 - t)])
        res = tm.feed(r, t); seq.append([t, r, res, tm.val, tm.at, tm.predict(t)]); t += rnd.choice([0.3, 0.5, 1.0])
    timers.append(seq)
breaks = []
for _ in range(4):
    b = F.BreakTracker(1234.5); seq, t, g = [], 0.0, 1.0
    for i in range(120):
        g = max(0.0, g - rnd.uniform(0.0, 0.05)) if b.phase == "drain" else min(1.0, g + 0.1)
        if rnd.random() < 0.05: g = None
        b.feed(g, rnd.choice(["white", "orange"]), t)
        info = b.info(t); seq.append([t, g, info["left"], info["phase"], info["rate"], info["rate_now"], info["breaks"], info["last_after"]])
        g = g if g is not None else 0.5
        t += 0.5
    breaks.append(seq)
with open(os.path.join(OUT, "fight_goldens.json"), "w", encoding="utf-8", newline="\n") as fh:
    json.dump({"prop": prop, "cases": cases, "timers": timers, "breaks": breaks}, fh)
print(len(cases), "fights,", sum(len(c["frames"]) for c in cases), "frames")
