"""Builds a synthetic game data folder (made-up pmdata.bin, LuaScripts.xdf and shipped CSVs, no real game data) and
writes what the reference gamedata.py (aLogs B16) reads from it, plus from byte-flipped copies, as goldens.

    python -I tools/gen_gamedata_goldens.py [reference/aLogs/bossbar]

Output: crates/alogs-gamedata/tests/data/game/... and crates/alogs-gamedata/tests/data/gamedata_goldens.json
"""
import io, json, os, random, struct, sys, zipfile

here = os.path.dirname(os.path.abspath(__file__))
src = sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, "..", "reference", "aLogs", "bossbar")
sys.path.insert(0, os.path.abspath(src))
import gamedata  # noqa: E402

OUT = os.path.join(here, "..", "crates", "alogs-gamedata", "tests", "data")
LUA = os.path.join(OUT, "game", "lua")
PM = os.path.join(LUA, "LuaScripts", "Common", "Data", "pmdata.bin")

# ---- a tiny encoder for the packed format gamedata.PmData decodes
TABLES = ["bossrush_cycle_data", "bossrush_guanka_data", "property_data", "puppet_data", "puppet_data1",
          "puppet_data2", "puppet_data3", "sys_config_data", "bossrush_grade_data"]
FIELDS = ["seasonId", "isRank", "begDayTime", "endDayTime", "bossLeft", "bossMid", "bossRight", "bossId", "bossId_2",
          "fixed_hp_rate", "species_hp_max_v", "propId", "monsterType", "bossType", "lvType", "uiName", "name",
          "BossShowName", "tags", "BossRushMaxStarSideBoss", "BossRushStarMultiplierMidBoss", "BossRushCombatDuration",
          "BossRushPlayerBotList", "bossHpLossPercent", "bossHpLossPercent_2", "bossHpLossPercent_3", "gradeName",
          "ALPHA", "BETA", "GAMMA", "Grove Boss", "1", "2", "3", "4", "5", "6"]
FILLER = [f"zz{i:04d}" for i in range(1000)]
STRINGS = sorted(set([""] + TABLES + FIELDS + FILLER))


class Enc:
    def __init__(self):
        self.b = bytearray()
        self.off = {}
        for s in STRINGS:
            self.off[s] = len(self.b); self.b += s.encode() + b"\0"
        self.b += b"\xff\0"                                  # ends the pool (not UTF-8)
        self.keysets = {}

    def here(self): return len(self.b)
    def put(self, data): at = len(self.b); self.b += data; return at
    def pad(self, n=3): self.b += b"\0" * n

    def keyset(self, names, a=0):
        key = (tuple(names), a)
        if key in self.keysets: return self.keysets[key]
        self.pad(); k = self.put(b"\0\0\0\0")
        self.b += b"\x01" + struct.pack("<II", len(names), a) + b"".join(struct.pack("<I", self.off[n]) for n in names)
        self.keysets[key] = k
        return k

    def numkeyset(self, nums, tag=5):
        self.pad(); k = self.put(b"\0\0\0\0")
        fmt = {0: "<d", 5: "<i", 3: "<H", 1: "<B"}[tag]
        self.b += bytes([tag]) + struct.pack("<II", len(nums), len(nums)) + b"".join(struct.pack(fmt, n) for n in nums)
        return k

    def obj(self, K, n, a, fields, consts=None):
        """fields: [(type, value)] for n + a slots; type 8 values are object offsets; consts {slot: (type, value)}."""
        consts = consts or {}
        types = bytes(0x64 + consts[i][0] if i in consts else fields[i][0] for i in range(n + a))
        A = self.put(types)
        cdata = b"".join(self.raw(*consts[i]) for i in range(n + a) if i in consts)
        self.put(cdata)
        body, offs = bytearray(), []
        for i in range(n + a):
            offs.append(4 + len(body))
            if i not in consts: body += self.raw(*fields[i])
        B = self.put(b"\x03" + b"".join(struct.pack("<H", o) for o in offs))
        self.pad(); t = self.put(struct.pack("<III", K, A, B))
        self.pad(); o = self.put(struct.pack("<I", t) + bytes(body))
        return o

    def raw(self, ty, v):
        if ty == 0: return struct.pack("<d", v)
        if ty == 1: return struct.pack("<B", v)
        if ty == 2: return struct.pack("<b", v)
        if ty == 3: return struct.pack("<H", v)
        if ty == 4: return struct.pack("<h", v)
        if ty == 5: return struct.pack("<i", v)
        if ty == 6: return struct.pack("<I", v)
        if ty == 7: return struct.pack("<I", self.off[v] if isinstance(v, str) else v)
        if ty == 8: return struct.pack("<I", v)
        if ty == 9: return bytes([1 if v else 0])
        raise ValueError(ty)

    def record(self, pairs, consts=None):
        names = [k for k, _ in pairs]
        return self.obj(self.keyset(names), len(names), 0, [v for _, v in pairs], consts)

    def numtable(self, rows, tag=5):
        """{number: object offset} as an object with a numeric key set."""
        nums = list(rows)
        K = self.numkeyset(nums, tag)
        return self.obj(K, len(nums), 0, [(8, rows[n]) for n in nums])


def build(seed=1):
    rnd = random.Random(seed)
    e = Enc()
    # bossrush_guanka_data: species -> stage
    stages = {}
    for sp in range(101, 111):
        pairs = [("bossId", (5, 500000 + sp))]
        if sp in (104, 105): pairs.append(("bossId_2", (5, 500100 + sp)))
        stages[sp] = e.record(pairs)
    guanka = e.numtable(stages)
    # bossrush_cycle_data: week -> schedule
    cycles = {}
    for w in range(3000, 3006):
        begd = e.obj(e.numkeyset([1, 2, 3], 1), 3, 0, [(5, 1760000000 + (w - 3000) * 604800), (5, 0), (5, 0)])
        endd = e.obj(e.numkeyset([1, 2, 3], 1), 3, 0, [(5, 0), (5, 0), (5, 1760000000 + (w - 2999) * 604800 - 1)])
        side = lambda: (5, rnd.choice(list(stages) + [999]))
        cycles[w] = e.record([("seasonId", (3, 3)), ("isRank", (3, 174)), ("begDayTime", (8, begd)),
                              ("endDayTime", (8, endd)), ("bossLeft", side()), ("bossMid", side()), ("bossRight", side())],
                             consts={1: (3, 91)} if w == 3005 else None)
    cyc = e.numtable(cycles)
    # property_data: propId -> fixed_hp_rate
    props = {}
    for pid in range(10, 20):
        f = rnd.choice([3, 3101, 3133, 3016, 3111])
        rate = e.record([("1", (6, f)), ("2", (0, round(rnd.uniform(1, 9), 3))), ("3", (0, 1.176)), ("4", (0, 1.2)),
                         ("5", (1, rnd.choice([0, 1])))])
        props[pid] = e.record([("fixed_hp_rate", (8, rate)), ("species_hp_max_v", (2, rnd.choice([0, -5, 7])))])
    prop = e.numtable(props)
    # puppet tables
    puppets = []
    for t in range(4):
        rows = {}
        for i in range(12):
            pid = 500101 + i if t == 0 else 600000 + 100 * t + i
            rows[pid] = e.record([("propId", (5, rnd.choice(list(props) + [0, 99]))), ("monsterType", (1, rnd.choice([2, 2, 1]))),
                                  ("bossType", (1, rnd.choice([0, 3]))), ("lvType", (1, 1)),
                                  ("uiName", (7, rnd.choice(["ALPHA", "BETA", "GAMMA", ""]))),
                                  ("name", (6, rnd.choice([1, 2, 3, 77]))), ("BossShowName", (5, rnd.choice([2, 3, -1]))),
                                  ("species_hp_max_v", (3, rnd.choice([95, 110, 300])))])
        puppets.append(e.numtable(rows))
    # sys_config_data
    botlist = e.obj(e.numkeyset([1, 2, 3], 1), 3, 0, [(5, 30), (5, 10), (5, 20)])
    cfg = e.record([("BossRushMaxStarSideBoss", (5, 20)), ("BossRushStarMultiplierMidBoss", (5, 2)),
                    ("BossRushCombatDuration", (5, 300)), ("BossRushPlayerBotList", (8, botlist))])
    # bossrush_grade_data
    grades = {}
    steps = sorted(rnd.random() for _ in range(5))
    for g, v in enumerate([0.0] + steps + [1.0]):
        pairs = [("bossHpLossPercent", (0, v)), ("gradeName", (7, "Grove Boss"))]
        if g % 2: pairs.append(("bossHpLossPercent_2", (0, v)))
        grades[g] = e.record(pairs)
    grade = e.numtable(grades, tag=3)
    # the directory: 1000+ keys, the tables among them
    names = sorted(TABLES + FILLER)
    tabs = {"bossrush_guanka_data": guanka, "bossrush_cycle_data": cyc, "property_data": prop,
            "puppet_data": puppets[0], "puppet_data1": puppets[1], "puppet_data2": puppets[2], "puppet_data3": puppets[3],
            "sys_config_data": cfg, "bossrush_grade_data": grade}
    e.record([(n, (8, tabs[n]) if n in tabs else (1, 0)) for n in names])
    e.pad(16)
    return bytes(e.b)


def xdf():
    text = b"Alpha OneOmega TwoGrove"
    where = {"1": [0, 9], "2": [9, 9], "3": [18, 5], str(0xffffffff): [0, 5]}
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("readme.txt", "synthetic")
        z.writestr("LuaScripts/i18n/Compress_en.bin", text)
        z.writestr("LuaScripts/i18n/NewTextMap_en.json", json.dumps(where))
    return buf.getvalue()


def run(fn):
    try: return {"ok": repr(fn())}
    except Exception as ex: return {"err": type(ex).__name__}


def main():
    os.makedirs(os.path.dirname(PM), exist_ok=True)
    base = build()
    with open(PM, "wb") as f: f.write(base)
    with open(os.path.join(LUA, "LuaScripts.xdf"), "wb") as f: f.write(xdf())
    catalog = os.path.join(OUT, "game", "boss_catalog.csv")
    grades_csv = os.path.join(OUT, "game", "boss_rush_grades.csv")
    keep = [("puppet_data1", "600101")]
    rows = gamedata.catalog_rows(PM, keep)
    rows[0] = dict(rows[0], name_en="Changed"); rows = rows[1:] + rows[:1]; del rows[3]
    import csv
    with open(catalog, "w", encoding="utf-8", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0])); w.writeheader(); w.writerows(rows)
    gamedata.write_grades(PM, grades_csv)
    with open(grades_csv, encoding="utf-8") as f: lines = f.read().splitlines()
    parts = lines[3].split(","); parts[3] = "0.5"; lines[3] = ",".join(parts)
    with open(grades_csv, "w", encoding="utf-8", newline="\n") as f: f.write("\n".join(lines) + "\n")
    cycles_json = os.path.join(OUT, "game", "boss_rush_cycles.json")
    with open(cycles_json, "w", encoding="utf-8") as f:
        json.dump([{"cycle": "3000", "bossIds": [500101, 500999, 500102], "bossIds2": [None, 500204, None]}], f)

    def report(path):
        pm = lambda: gamedata.PmData(path)
        out = {name: run(lambda name=name: pm().table(name)) for name in TABLES + ["nope"]}
        out["read_cycles"] = run(lambda: gamedata.read_cycles(path))
        out["read_grades"] = run(lambda: gamedata.read_grades(path))
        out["read_rush_rules"] = run(lambda: gamedata.read_rush_rules(path))
        out["boss_rush_ids"] = run(lambda: sorted(gamedata.boss_rush_ids(path)))
        return out

    golden = {"base": report(PM)}
    golden["base"]["read_bosses"] = run(lambda: gamedata.read_bosses(PM, keep))
    golden["base"]["catalog_rows"] = run(lambda: gamedata.catalog_rows(PM, keep))
    golden["base"]["catalog_drift"] = run(lambda: gamedata.catalog_drift(PM, catalog))
    golden["base"]["catalog_drift_none"] = run(lambda: gamedata.catalog_drift(None, catalog))
    golden["base"]["grades_drift"] = run(lambda: gamedata.grades_drift(PM, grades_csv))
    golden["base"]["shipped_grades"] = run(lambda: gamedata.shipped_grades(grades_csv))
    golden["base"]["covers"] = [gamedata.covers_now(gamedata.read_cycles(PM), now) for now in
                                (1759999999, 1760000000, 1760300000, 1763628799, 1763628800)]

    # byte-flipped copies: both must read the same or both fail
    rnd = random.Random(7)
    tmp = os.path.join(OUT, "fuzz.bin")
    golden["fuzz"] = []
    for i in range(150):
        flips = [[rnd.randrange(len(STRINGS) * 4, len(base)), rnd.randrange(256)] for _ in range(rnd.choice([1, 2, 4, 8]))]
        b = bytearray(base)
        for at, v in flips: b[at] = v
        with open(tmp, "wb") as f: f.write(b)
        golden["fuzz"].append({"flips": flips, "report": report(tmp)})
    os.remove(tmp)
    dst = os.path.join(OUT, "gamedata_goldens.json")
    with open(dst, "w", encoding="utf-8", newline="\n") as f: json.dump(golden, f, indent=0)
    print("wrote", os.path.normpath(dst), os.path.getsize(dst), "bytes;", len(base), "byte pmdata.bin")


main()
