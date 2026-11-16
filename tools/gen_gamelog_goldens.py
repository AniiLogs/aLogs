"""Writes crates/alogs-gamelog/tests/data/{made_up.log,gamelog_goldens.json} from the reference core.py (aLogs B16):
LogWatcher events fed whole, line by line and in small pieces, look_back, last_uid, scene_of, interlink_score and
same_name. The log is made up from the patterns the watcher looks for (no player's log), as in
tests/log_watcher_same.py. Times are UTC (time.mktime = calendar.timegm) and "now" is fixed, so both sides agree.

    python -I tools/gen_gamelog_goldens.py [reference/aLogs/bossbar]
"""
import calendar, collections, json, os, random, sys, time

here = os.path.dirname(os.path.abspath(__file__))
src = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, "..", "reference", "aLogs", "bossbar"))
sys.path.insert(0, src)
import logging
logging.disable(logging.CRITICAL)
import core  # noqa: E402
from core import LogWatcher  # noqa: E402

OUT = os.path.join(here, "..", "crates", "alogs-gamelog", "tests", "data")
NOW = 1_800_000_000 + 1500.0
time.mktime = calendar.timegm
time.time = lambda: NOW


def made_up_log(n=3000, seed=7):
    rnd = random.Random(seed); t0 = 1_800_000_000; out = []
    for i in range(n):
        ts = time.strftime('%Y-%m-%d %H:%M:%S', time.gmtime(t0 + i)) + f'.{rnd.randint(0, 999):03d}'
        stamp = rnd.choice([f'[{ts}]', f'[{ts}][info]', f'[{ts}][warn]', '', '[2026-13-01 00:00:00.5]', '[2026-02-30 25:00:00.1]'])
        uid = rnd.choice(['100000000001', '100000000002', '123456', '12345'])
        space = f'{rnd.randint(1, 9)}-{rnd.choice(["100000000001", "200000000002"])}-{rnd.choice([550, 1201, 3301])}'
        body = rnd.choice([
            f'net request callerId={uid} op=Get',
            f'download https://cdn.example/prod/ClientInfo/{uid}_client_info_bin ok',
            f'download https://cdn.example/prod/pet_jewelry/{uid} ok',
            'download https://cdn.example/prod/other/file ok',
            f'createVoxelScene new>>>id:{space}',
            f'createVoxelScene new>>>id:{space} callerId={uid}',
            f'clearSpace is_destroy:true id:{space}',
            f'clearSpace is_destroy:false id:{space}',
            'load scene fb_bossrush_03',
            'load scene fb_roguelike_holo',
            'bossrush ui opened',
            'plain line with nothing in it',
            'info about ][info] in the middle',
            'unicode é中 text createVoxelScene new>>>id:',
            'createVoxelScene new>>>id:tab\there',
            '',
        ])
        out.append(f'{stamp} {body}' if stamp else body)
    text = '\n'.join(out).encode('utf-8')
    text = text.replace(b'plain line', b'pl\xffain\xe2\x82 line', 40)
    return text + b'\ncreateVoxelScene new>>>id:unfinished'


class Events(list):
    def put(self, e): self.append(e)


def run(log_bytes, pieces, prefill=0):
    w = LogWatcher.__new__(LogWatcher)
    w.events = Events(); w.uid = None; w.current = None; w.delays = collections.deque(maxlen=20); w.buf = b''
    for _ in range(prefill): w.delays.append(0.0)
    for p in pieces: w.feed(p)
    return {"events": [list(e) for e in w.events], "uid": w.uid.decode() if w.uid else None, "current": w.current,
            "delays": list(w.delays), "buf": w.buf.decode("latin-1")}


def small(b, seed):
    rnd = random.Random(seed); out = []; i = 0
    while i < len(b):
        n = rnd.randint(1, 120); out.append(b[i:i + n]); i += n
    return [len(x) for x in out]


log_bytes = made_up_log()
os.makedirs(OUT, exist_ok=True)
with open(os.path.join(OUT, "made_up.log"), "wb") as f: f.write(log_bytes)
lines = [x + b'\n' for x in log_bytes.split(b'\n')[:-1]] + [log_bytes.split(b'\n')[-1]]
g = {"now": NOW, "feeds": []}
cuts = {"whole": [len(log_bytes)], "lines": [len(x) for x in lines], "small1": small(log_bytes, 1), "small2": small(log_bytes, 2)}
for name, sizes in cuts.items():
    pieces, at = [], 0
    for n in sizes: pieces.append(log_bytes[at:at + n]); at += n
    for prefill in (0, 15, 20):
        g["feeds"].append({"cut": name, "sizes": sizes, "prefill": prefill, "result": run(log_bytes, pieces, prefill)})

w = LogWatcher.__new__(LogWatcher); w.events = Events(); w.current = None; w.path = os.path.join(OUT, "made_up.log")
for size, span in ((len(log_bytes), 16 << 20), (len(log_bytes), 5000), (90000, 3000)):
    w.events = Events(); w.current = None
    w.look_back(size, span)
    g.setdefault("look_back", []).append({"size": size, "span": span, "events": [list(e) for e in w.events], "current": w.current})
g["last_uid"] = core.last_uid(os.path.join(OUT, "made_up.log"))
with open(os.path.join(OUT, "spaces.log"), "wb") as f:
    f.write(b"x createVoxelScene new>>>id:1-1234567-550\ny createVoxelScene new>>>id:2-7654321-3000\nnone here")
g["last_uid_spaces"] = core.last_uid(os.path.join(OUT, "spaces.log"))
g["scene_of"] = [[s, core.scene_of(s)] for s in ["100-123-550", "100-123-3000", "1-2", "a-1-2", "1-2-3-4", "", None, "7-8-09"]]
rnd = random.Random(3)
cases = [(12058, 12, None, False), (17164, 17, None, False), (77764, 17, None, False), (21519, 20, 20, False),
         (21519, 20, None, False), (73419, 13, None, False), (None, 3, None, False), (5000, None, None, False),
         (9999, 3, None, True), (777, 0, None, False), (7777, 1, 1, False), (0, 0, None, False)]
cases += [(rnd.choice([rnd.randint(0, 99999), int(''.join(rnd.choice('1777') for _ in range(5)))]), rnd.randint(0, 40),
           rnd.choice([None, 20, 40]), rnd.random() < 0.1) for _ in range(300)]
g["interlink_score"] = [[s, st, cap, two, core.interlink_score(s, st, cap, two)] for s, st, cap, two in cases]
names = ["Kyhra", "Kyhru", "kyhra ", "KYHRA", "Abc", "Abd", "Straße", "STRASSE", "Moonlight", "Moonlite", "MoonIight",
         "", None, "  ", "Lunaria", "Lunarla", "xXSlayerXx", "xXS1ayerXx", "aaaa", "aaab", "Mia", "Mia", "Élise", "elise",
         "ab" * 120, "ab" * 119 + "ba"]
g["same_name"] = [[a, b, core.same_name(a, b)] for a in names for b in names]
from difflib import SequenceMatcher
pairs = [("".join(rnd.choice("abcab ") for _ in range(rnd.randint(0, 260))), "".join(rnd.choice("abcab ") for _ in range(rnd.randint(0, 260)))) for _ in range(60)]
g["ratio"] = [[a, b, SequenceMatcher(None, a, b).ratio()] for a, b in pairs]
with open(os.path.join(OUT, "gamelog_goldens.json"), "w", encoding="utf-8", newline="\n") as f: json.dump(g, f)
print("wrote goldens:", sum(len(f["result"]["events"]) for f in g["feeds"]), "events over", len(g["feeds"]), "feeds")
