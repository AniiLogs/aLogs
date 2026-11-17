"""Writes crates/alogs-protocol/tests/data/protocol_goldens.json from the reference settings.py, plugins.py and
notice_window.py's take_notices (aLogs B16), on made-up settings and notice files.

    python -I tools/gen_protocol_goldens.py [reference/aLogs/bossbar]
"""
import json, os, re, sys, tempfile, textwrap

here = os.path.dirname(os.path.abspath(__file__))
src = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, "..", "reference", "aLogs", "bossbar"))
tmp = tempfile.mkdtemp()
os.environ["ALOGS_DATA"] = tmp
sys.path.insert(0, src)
import settings as C  # noqa: E402
import plugins as P  # noqa: E402

out = {"settings": [], "automation": [], "plugins": [], "health": [], "notices": []}

STORED = [
    None, "not json", "[1, 2]" and "{}", '{"meter": {"enabled": false}, "extra": 1}',
    '{"overlay": {"hide_from_capture": true, "world_position": "quests", "backdrop": 40}}',
    '{"overlay": {"hide_from_capture": false, "in_recordings": false}}',
    '{"overlay": "x", "capture": {"keep_hours": 5, "nope": 1}, "user_set": ["capture.keep_hours"]}',
    '{"leaderboard": {"notify_place": "left", "notify_locked": false, "token": "t"}, "notifications": {"place": "right"}}',
    '{"leaderboard": {"notify_motion": "drop"}, "notifications": 3}',
    '{"leaderboard": {"submit": true, "automation": true}, "results": {"sim_gate": true}, "app": {"priority": "high"}}',
    '{"perf": {"look_interval": 0.1, "fight_rate": 60.0}, "meter": 5, "game": {"window_title": "Ani\\u00efmo"}}',
]
for text in STORED:
    if os.path.exists(C.SETTINGS_FILE): os.remove(C.SETTINGS_FILE)
    if text is not None:
        with open(C.SETTINGS_FILE, "w", encoding="utf-8") as f: f.write(text)
    cfg = C.load()
    C.save(cfg)
    with open(C.SETTINGS_FILE, encoding="utf-8") as f: saved = f.read()
    out["settings"].append({"stored": text, "load": repr(cfg), "save": saved})
    for what in (None, "sim_gate", "sim_gizmos", "nope"):
        out["automation"].append({"cfg": json.loads(saved), "what": what, "on": C.automation(cfg, what)})

CFGS = [{}, {"sanctum": {"enabled": False}}, {"leaderboard": {"submit": True}},
        {"leaderboard": {"submit": 1, "notify_sent": False}, "notifications": {"button": 0}},
        {"leaderboard": {"submit": True, "notify_sent": None}, "sanctum": {"enabled": "yes"}, "notifications": {"button": False}}]
for cfg in CFGS:
    for meter_on in (True, False):
        for failed in ((), ("leaderboards",), ("dps", "sanctum")):
            P.HEALTH = P.Health()
            for pid in failed: P.HEALTH.stopped[pid] = "x"
            out["plugins"].append({
                "cfg": cfg, "meter_on": meter_on, "failed": list(failed),
                "switched_on": {pid: P.switched_on(cfg, pid, meter_on) for pid in P.PLUGINS},
                "notice_owners": repr(P.notice_owners(cfg, meter_on)),
                "meter_mode": P.meter_mode(not meter_on, cfg)})

# Health: errors at given times, then status
import logging
logging.disable(logging.CRITICAL)
for times in ([0, 10, 20], [0, 30, 61, 62], [0, 59, 59.5, 120, 121, 122], [5]):
    h = P.Health(); seq = []
    real = P.time.time
    for t in times:
        P.time.time = lambda t=t: t
        h.error("leaderboards", ValueError("bad 'x'"), "send", )
        seq.append({"failed": h.failed("leaderboards"), "status": repr(h.status("leaderboards"))})
    P.time.time = real
    h.reset("leaderboards"); seq.append({"failed": h.failed("leaderboards"), "status": repr(h.status("leaderboards"))})
    out["health"].append({"times": times, "steps": seq})

# notices: take_notices, lifted from notice_window.py and run on files
srcfile = open(os.path.join(src, "notice_window.py"), encoding="utf-8").read()
body = srcfile[srcfile.index("    def take_notices(now):"):srcfile.index("    def toast_tick():")]
ns = {"os": os, "json": json, "re": re, "NOTICES": os.path.join(tmp, "notices"),
      "toast": {"queue": []}}
exec(textwrap.dedent(body), ns)
NOTES = [
    '{"head": "Nest found", "detail": "Room 3", "accent": "#12abEF", "screen": [0, 0, 1920, 1080], "from": "sanctum"}',
    '{"head": "' + "x" * 130 + '", "detail": null, "accent": "red", "screen": [1.9, "2", 3, 4]}',
    '{"head": 12, "detail": 3.5, "accent": "#1234567", "screen": [0, 0, 0, 10], "from": ""}',
    '{"head": "", "detail": "no head"}',
    '{"head": "bad screen", "screen": [0, 0, "a", 1]}',
    '{"head": "bad int", "screen": [0, "z", 1, 1]}',
    '[1, 2]',
    'nope',
    '{"head": "three", "screen": [1, 2, 3]}',
    '{"head": "accent none", "accent": null, "from": "button"}',
    '{"head": true, "detail": false, "screen": [true, false, true, true]}',
]
os.makedirs(ns["NOTICES"], exist_ok=True)
for text in NOTES:
    with open(os.path.join(ns["NOTICES"], "00000000000000000001-1.json"), "w", encoding="utf-8") as f: f.write(text)
    ns["toast"] = {"queue": []}
    try:
        ns["take_notices"](1000.0)
        got = ns["toast"]["queue"]
        out["notices"].append({"text": text, "queue": [dict(n, screen=list(n["screen"]) if n["screen"] else None) for n in got]})
    except Exception as ex:
        out["notices"].append({"text": text, "raised": type(ex).__name__})

dst = os.path.join(here, "..", "crates", "alogs-protocol", "tests", "data", "protocol_goldens.json")
os.makedirs(os.path.dirname(dst), exist_ok=True)
with open(dst, "w", encoding="utf-8", newline="\n") as f: json.dump(out, f, indent=0)
print("wrote", os.path.normpath(dst), os.path.getsize(dst), "bytes")
