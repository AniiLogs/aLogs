"""A two-boss middle fight (weeks 4003-4005, "One-after-one") through the meter's own fight path, from scripted runs.

    python tests/two_boss.py [-v] [--only a,b,...] [--trace FILE] [--no-checks]

Nobody has seen this fight live yet, so there are no captures to replay. Each scenario is a timeline of what the game
would show: the boss bar (its boss, xN counter and fill), the name above it, the clock, and the result screen. The
meter runs its real Meter.step() on it: the bar lock and grace, identify, start_fight, Fight.update, end_fight, the
screen reads, result_screen and fight_summary. Only the reads of the screen itself are stubbed: the bar finder,
fill and break readers, the xN digit matcher and OCR (the readers answer from the timeline). The bar's text reads
come back ~0.13 s later and the result screen's name read ~0.4 s later, as the background readers deliver them live.

How the fight goes comes from the game files (build 3857679): the first boss (bossrush_guanka_data bossId) on
bossHpLossPercent_2 and the second (bossId_2) on bossHpLossPercent_3 (BossRushUtils getBossHpLossPercentField, line
306); one 180 s clock (240 s with Overtime) that starts with the first boss and never restarts, the second boss
spawning 2 s after the first one's death (level graph Scene.1070 scene_graph_data[78824533], nodes 5-11 and 16-19);
the HUD hidden for 2.8 s after a boss dies, the bar may stay or vanish (BossTitleItemV2.refreshBossTitle, line 345);
the second boss back at x21 (BossTitleItemV2.switchBossRushTarget, line 484). The max HP of each is the meter's own
prediction at Lv 60. The gaps, the paces and the stars and scores shown are made up: the server's star and score
rules for this fight are unknown, and nothing here assumes one.

Scenarios: a) double kill at 150 s; b) the first boss killed, the second timing out at 180 s, the bar never vanishing;
c) Overtime (240 s), the second boss killed at 230 s; d) the first boss timing out, so the second never comes;
e) a wipe during the second boss, after a 25 s gap with no game-log Interlink scene; f1/f2) one-boss fights on a side
slot (week 4003) and on a pre-4003 centre (week 4002); g1/g2) a boss that isn't the partner showing up, in a two-boss
fight and in a one-boss fight: it ends the fight as before; h) the meter joining at the second boss, whose clock
then shows what is left of the run's, which is no time limit; i) the second boss's bar first seen past its top bars,
so its name hands over.

Nothing is sent and nothing lands in your logs: the meter's data folder is a temporary one. --trace writes what the
meter put out per scenario (statuses, fight CSV rows, summaries) to compare two checkouts (ALOGS_BOSSBAR picks the
meter's folder; --no-checks for one without the two-boss code). Exit code 1 on any failure.
"""
import argparse, json, logging, os, shutil, sys, tempfile, threading, time, types
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
TMP = tempfile.mkdtemp(prefix='alogs_twoboss_')
os.environ['ALOGS_DATA'] = TMP                  # before settings is imported: a fresh data folder
REAL_TIME = time.time
from PIL import Image
import live, reader as R, xn_digits as X, settings as C

ap = argparse.ArgumentParser()
ap.add_argument('-v', action='store_true', help="print the meter's log")
ap.add_argument('--only', help='scenarios to run, comma-separated (a,b,c,d,e,f1,f2,g1,g2,h,i)')
ap.add_argument('--trace', help='write what the meter put out to this JSON file')
ap.add_argument('--no-checks', action='store_true', help='only run (for a checkout without the two-boss code)')
ARGS = ap.parse_args()

W, H = 1280, 720
SPAN = R.REF_SPAN * H / 1125
LB = (W - SPAN) / 2
BAR = {'yc': round(0.055 * H), 'left_bracket': LB, 'span': SPAN, 'brackets_found': True, 'world': False, 'right_red': 0.0,
       'track_left': round(LB + R.TRACK[0] * SPAN), 'track_right': round(LB + R.TRACK[1] * SPAN), 'thickness': 12, 'frame': (W, H)}
LIVE_RED, NO_HP = (240, 110, 100), (70, 70, 80)       # live HP at the bar's left end; an empty or dead bar
DT = 1 / 30                                           # the meter's pace during a fight (FIGHT_INTERVAL)
LEVEL = 60                                            # bossrush_cycle_data[4003..4005].bossLv; this season's bosses show Lv. 60
CATALOG = R.load_catalog()
ROW = {r['puppetId']: r for r in CATALOG if r['table'] == 'puppet_data' and r['fixed_hp_rate']}
GRADES = R.load_grades()

def week_start(cycle):
    with open(R.CYCLES_FILE[0], encoding='utf-8') as f: cycles = json.load(f)
    return next(c for c in cycles if str(c['cycle']) == str(cycle))['start']

def bar_state(frac, cum):
    """(xN, fill) of a boss that has lost `frac` of its max HP, on its grade column `cum` (22 rows: x21 .. x1)."""
    top = len(cum) - 1
    for g in range(1, top + 1):
        if frac < cum[g] or g == top:
            lo, hi = cum[g - 1], cum[g]
            return len(cum) - g, max(0.0, min(1.0, 1 - (frac - lo) / (hi - lo)))

class Script:
    """A run as the game would show it, t seconds after its clock started. Pieces, in order:
    ('boss', pid, t0, t1, dps): the bar on that boss from t0 to t1, losing dps HP a second (the first bar the meter
    sees shows a second before that, at full HP: before the clock starts, or before a late-joining meter's boss is hit);
    ('dead', pid, t0, t1): its bar at x1, empty, the HUD's timer hidden (the 2.8 s after a boss dies);
    ('hidden', pid, t0, t1, dps): no bar (between the bosses, an ultimate, the run's end), pid losing dps HP a second;
    ('result', None, t0, t1): the result screen.
    ocr_lag: looks before a text read of the bar comes back (OcrWorker: ~0.1 s live)."""
    def __init__(self, week, clock=180, log_scene=True, ocr_lag=4):
        self.week, self.clock, self.log_scene, self.ocr_lag = week, clock, log_scene, ocr_lag
        self.parts = []; self.titles = []; self.result = None
    def boss(self, pid, t0, t1, dps, kill=False):
        if not self.parts: self.parts.append(('boss', pid, t0 - 1.0, t0, 0.0))   # in view a second before it's hit
        self.parts.append(('boss', pid, t0, t1, dps))
        if kill: assert abs(self.lost(pid, t1) - self.hp(pid)) < 1, f'{pid} not down at {t1}: the script is wrong'
        return self
    def dead(self, pid, t0, t1): self.parts.append(('dead', pid, t0, t1, 0)); return self
    def hidden(self, t0, t1, pid=None, dps=0):
        self.parts.append(('hidden', pid, t0, t1, dps)); return self             # (a boss can take hits meanwhile)
    def show_result(self, t0, t1, stars, score, secs):
        self.parts.append(('result', None, t0, t1, 0)); self.result = {'stars': stars, 'score': score, 'time': secs}; return self
    def title(self, t0, t1, text): self.titles.append((t0, t1, text)); return self
    @property
    def end(self): return self.parts[-1][3]

    def hp(self, pid): return R.boss_max_hp(ROW[pid], LEVEL, use_calibration=False)
    def lost(self, pid, t):
        """HP the boss has lost by t (its pieces before t)."""
        out = 0
        for kind, p, t0, t1, dps in self.parts:
            if kind in ('boss', 'hidden') and p == pid and t0 <= t: out += dps * (min(t, t1) - t0)
        return min(out, self.hp(pid))
    def at(self, t):
        part = next((p for p in self.parts if p[2] <= t < p[3]), ('hidden', None, t, t, 0))
        kind, pid = part[0], part[1]
        s = {'kind': kind, 'pid': pid, 'bar': kind in ('boss', 'dead'), 'timer': None, 'title': None, 'xn': None,
             'fill': None, 'result': kind == 'result'}
        if pid and kind in ('boss', 'dead'):
            row = ROW[pid]; cum = GRADES[R.grade_column(pid)]
            s['title'] = f"Lv. {LEVEL} {row['bossTitle_en']}"
            if kind == 'boss':
                s['xn'], s['fill'] = bar_state(self.lost(pid, t) / self.hp(pid), cum)
                left = self.clock - max(0.0, t)
                s['timer'] = f'{int(left) // 60}:{int(left) % 60:02d}' if left >= 0 else None
            else: s['xn'] = 1
        for t0, t1, text in self.titles:
            if t0 <= t < t1: s['title'] = text
        return s

class Game:
    """The screen readers' stand-ins: each answers what the timeline shows at the fake clock's time."""
    def __init__(self, script, t_start):
        self.script, self.t_start = script, t_start; self.now = t_start; self.reads = []
    @property
    def t(self): return self.now - self.t_start
    def state(self): return self.script.at(self.t)

class FakeOcr(R.NoOcr):
    """OCR: the bar's title, timer and xN, the screens' headers and the result screen, from the timeline."""
    def read(self, images, names=None, raws=None):
        s = GAME.state(); out = {}
        res = GAME.script.result if s['result'] else None
        for k in images:
            v = ''
            if k == 'title': v = s['title'] or ''
            elif k == 'timer': v = s['timer'] or ''
            elif k == 'xn': v = ' / '.join([f"x{s['xn']}"] * 3) if s['xn'] else ''
            elif res and k == 'lbl': v = 'Challenge Rating Challenge Score Completion Time'
            elif res and k == 'rating': v = ' '.join([str(res['stars'])] * 3)
            elif res and k == 'score': v = str(res['score'])
            elif res and k == 'time': v = ' '.join([f"{res['time']}s"] * 3)
            out[k] = v
        GAME.reads.append((round(GAME.t, 2), threading.current_thread().name, {k: v for k, v in out.items() if k in ('title', 'xn', 'rating', 'score', 'time') and v}))
        return out
    def cached(self, images, names): return False      # every name read goes through the names worker, as a first read
    def close(self): pass

class LagWorker:
    """OcrWorker's stand-in: the text is read from the frame it was given (now), and handed over `lag` looks later."""
    def __init__(self, ocr, lag): self.ocr, self.lag = ocr, lag; self.job = None; self.result = None; self.left = 0
    @property
    def busy(self): return self.job is not None
    def submit(self, crops, meta, **kw):
        if self.job is None: self.job = (self.ocr.read(crops, **kw), meta); self.left = self.lag
    def tick(self):
        if self.job is not None:
            self.left -= 1
            if self.left <= 0: self.result, self.job = self.job, None
    def take(self): r, self.result = self.result, None; return r
    def close(self): pass

def stub_readers():
    def find(im, *a, **k):
        if GAME.state()['bar']: R.LAST_FAIL[0] = ''; return dict(BAR)
        R.LAST_FAIL[0] = 'no red/orange run near the top'; return None
    R.find_bar = find; R.bar_at = lambda im, spots: find(im)
    R.verify_bar = lambda im, bar: GAME.state()['bar']
    def fill(im, bar):
        s = GAME.state()
        if s['kind'] == 'boss' and s['fill'] and s['fill'] > 0: return s['fill'], LIVE_RED, LIVE_RED
        return 0.0, NO_HP, NO_HP
    R.measure_fill_any = fill
    R.measure_break = lambda im, bar: (None, None, None)
    # the digit matcher reads every counter but those with an 8 (xn_templates.json has no 8): those fall back to OCR
    X.read_counter = lambda crop, tpl, top=None: (GAME.state()['xn'] if GAME.state()['xn'] and '8' not in str(GAME.state()['xn']) else None)
    X.load_templates = lambda: None
    live.load_korean_names = lambda *a, **k: None
    R.Ocr = FakeOcr

class Clock:
    def __init__(self): self.t = REAL_TIME()
    def __call__(self): return self.t

def run(name, script, uid='uid-test'):
    """The script through Meter.step(), one look every DT. Returns what the meter did."""
    global GAME
    shutil.rmtree(os.path.join(TMP, 'logs'), ignore_errors=True)
    t_start = week_start(script.week) + 3 * 86400 + 3600        # inside the week (the schedule's region-1 start + 3 days)
    GAME = Game(script, t_start)
    clock = Clock(); clock.t = t_start - 1.0; time.time = clock
    try: return play(name, script, uid, t_start, clock)
    finally: time.time = REAL_TIME

def play(name, script, uid, t_start, clock):
    cfg = C.load()
    cfg['capture'].update(end_screens=False, nobar_frames=False, debug_frames=False, test_frames=False)
    args = types.SimpleNamespace(cfg=cfg, replay=['two-boss timeline'], interval=0.05, title='Aniimo', duration=180,
                                 level=None, boss_id=None, player_log=os.path.join(TMP, 'Player.log'), no_overlay=True,
                                 hide_from_capture=False, no_debug_frames=True, no_auto_click=True, no_submit=True)
    outs = []
    def out(kind, data): outs.append((round(GAME.t, 3), kind, data))
    m = live.Meter(args, out)
    frame = Image.new('RGB', (W, H), (96, 104, 112))
    def grab(*a, **k):
        m.frame_size = (W, H); m.frame_full = True; return frame
    m.grab = grab; m.name_reader = lambda: None; m.picture = lambda *a, **k: None
    # the screens' text boxes: every one goes to OCR (on a real frame a box with no ink in it is skipped)
    m.screen_crops = lambda im, quick: {n: frame for n, kind, box in m.SCREEN_BOXES if not quick or n in m.QUICK_BOXES}
    m.ocr_bg = LagWorker(m.ocr, script.ocr_lag); m.names_bg = LagWorker(m.ocr, 12)
    m.uid = uid; m.rush_level = LEVEL
    space = '7700112233'                                     # an Interlink instance: no scene number in its id
    m.events.put(('enter', space, None))
    if script.log_scene: m.events.put(('bossrush', space, None))
    calls = {'next_boss': [], 'identify': []}
    if hasattr(live.Fight, 'next_boss'):
        orig_next = live.Fight.next_boss
        def next_boss(f, row, why):
            calls['next_boss'].append((round(GAME.t, 2), row['puppetId'], why, threading.current_thread() is threading.main_thread()))
            return orig_next(f, row, why)
        live.Fight.next_boss = next_boss
    orig_identify = m.identify
    def identify(title_text, xn):
        before = (m.fight, len(getattr(m.fight, 'phases', [None]))) if m.fight else None
        orig_identify(title_text, xn)
        calls['identify'].append((round(GAME.t, 2), title_text, before, list(m.switch_votes)))
    m.identify = identify
    fights, frames = [], []
    t = -1.0
    while t < script.end:
        clock.t = t_start + t; GAME.now = clock.t
        for w in (m.ocr_bg, m.names_bg): w.tick()
        force = getattr(script, 'force_title', None)
        if force and force[0] <= t < force[1] and not m.ocr_bg.busy and not getattr(script, 'forced', None):
            script.forced = t; m.last_title = 0.0; m.ocr_bg.lag = force[2]     # a title read cut now, slow to come back
        m.step()
        if getattr(script, 'forced', None) is not None: m.ocr_bg.lag = script.ocr_lag
        if m.fight and (not fights or fights[-1] is not m.fight): fights.append(m.fight)
        if m.fight: frames.append((round(t, 3), m.fight, m.fight.elapsed, m.fight.damage, m.fight.last_at == clock.t))
        t += DT
    for _ in range(40):                                       # let the late readers finish, the screens gone
        clock.t += DT; GAME.now = clock.t
        for w in (m.ocr_bg, m.names_bg): w.tick()
        m.step()
    m.end_fight('meter closed')                               # as the meter does on closing: a fight still open ends
    if hasattr(live.Fight, 'next_boss'): live.Fight.next_boss = orig_next
    summaries = {}
    d = os.path.join(TMP, 'logs', 'fights')
    for n in sorted(os.listdir(d)) if os.path.isdir(d) else []:
        with open(os.path.join(d, n), encoding='utf-8') as fh: summaries[n] = json.load(fh)
    csvs = {}
    for n in sorted(os.listdir(os.path.join(TMP, 'logs'))):
        if n.endswith('.csv') and n not in ('results.csv', 'time_check.csv'):
            with open(os.path.join(TMP, 'logs', n), encoding='utf-8') as fh: csvs[n] = fh.read().splitlines()
    for n, s in summaries.items():
        if s.get('result'): m.beats_best(dict(s))
    try:
        with open(os.path.join(TMP, 'logs', 'bests.json'), encoding='utf-8') as fh: bests = json.load(fh)
    except OSError: bests = {}
    return types.SimpleNamespace(name=name, script=script, meter=m, outs=outs, fights=fights, frames=frames, calls=calls,
                                 summaries=summaries, csvs=csvs, bests=bests, reads=GAME.reads)

# ---------------------------------------------------------------- the scenarios
BLAZEN, TUCKING = '80000120', '80000119'           # week 4003's middle: bossId, bossId_2 (bossrush_guanka_data 2000001)
GLAMEEP2, JABSTER2 = '80000122', '80000121'        # week 4004's (2000002)
GRIZBO2, JEBURST2 = '80000124', '80000123'         # week 4005's (2000003)
SHERRO, BOULDUS = '80000109', '80000101'           # week 4003's left and right bosses
MAGMAREX = '80000111'                              # week 4002's centre (one boss)

def hp(pid): return R.boss_max_hp(ROW[pid], LEVEL, use_calibration=False)

def scenarios():
    S = {}
    # a) double kill at 150 s: the first boss dies at t1, its dead bar for 2.8 s, no bar for 3.2 s, the second boss at
    #    x21 6 s after the first one's death; killed at 150 s; the result screen 3 s later
    h1, h2 = hp(BLAZEN), hp(TUCKING); dps = (h1 + h2) / (150 - 6); t1 = h1 / dps
    S['a'] = (Script('4003').boss(BLAZEN, 0, t1, dps, kill=True).dead(BLAZEN, t1, t1 + 2.8).hidden(t1 + 2.8, t1 + 6)
              .boss(TUCKING, t1 + 6, 150, dps, kill=True).dead(TUCKING, 150, 152.8).hidden(152.8, 153)
              .show_result(153, 160, 40, 52318, 150))
    # b) the first boss killed at 40 s; the second picked up 2.4 s later, before the 2.8 s are over, so the bar never
    #    vanishes; the clock runs out at 180 s. The score read '37250' with 31 stars: a 1 read as a 7 by the one-boss
    #    rule (31250), kept as read on a two-boss run. A title read is cut 0.3 s before the second boss shows: it names
    #    the first boss and arrives after the hand-off
    dps1 = hp(BLAZEN) / 40
    b = (Script('4003').boss(BLAZEN, 0, 40, dps1, kill=True).dead(BLAZEN, 40, 42.4)
         .boss(TUCKING, 42.4, 180, hp(TUCKING) * 0.7 / 137.6).hidden(180, 181).show_result(181, 188, 31, 37250, 180))
    b.force_title = (42.2, 42.39, 15); S['b'] = b       # cut in this window, handed back 15 looks (0.5 s) later
    # c) Overtime: a 4:00 clock. The first boss down at 35 s, an ultimate hides the bar for 5 s before that, no bar for
    #    12 s after its 2.8 s; the second boss killed at 230 s
    h1, h2 = hp(BLAZEN), hp(TUCKING); dps2 = h2 / (230 - 49.8)
    S['c'] = (Script('4003', clock=240).boss(BLAZEN, 0, 20, h1 / 30).hidden(20, 25).boss(BLAZEN, 25, 35, h1 / 30, kill=True)
              .dead(BLAZEN, 35, 37.8).hidden(37.8, 49.8).boss(TUCKING, 49.8, 230, dps2, kill=True).dead(TUCKING, 230, 232.8)
              .show_result(233, 240, 40, 49960, 230))
    # d) week 4004: the first boss (Alpha Glameep, 80000122) times out at 180 s with 70% of its HP gone
    S['d'] = (Script('4004').boss(GLAMEEP2, 0, 180, hp(GLAMEEP2) * 0.7 / 180).hidden(180, 181)
              .show_result(181, 188, 18, 18300, 180))
    # e) week 4005 with no Interlink scene from the game log: the first boss down at 30 s, no bar for 25 s (more than
    #    the 20 s grace outside a log-confirmed Interlink scene), the team wiped at 120 s
    h1 = hp(GRIZBO2)
    S['e'] = (Script('4005', log_scene=False).boss(GRIZBO2, 0, 30, h1 / 30, kill=True).dead(GRIZBO2, 30, 32.8).hidden(32.8, 55)
              .boss(JEBURST2, 55, 120, hp(JEBURST2) * 0.5 / 65).hidden(120, 122).show_result(122, 129, 27, 27800, 120))
    # f1) week 4003's left boss alone (Omega Sherro), killed at 100 s
    S['f1'] = (Script('4003').boss(SHERRO, 0, 100, hp(SHERRO) / 100, kill=True).dead(SHERRO, 100, 102.8).hidden(102.8, 103)
               .show_result(103, 110, 20, 26450, 100))
    # f2) week 4002's centre (Alpha Magmarex, one boss), timing out at 180 s; the score read '73419' with 13 stars is
    #     repaired to 13419 as before (a 1 read as a 7)
    S['f2'] = (Script('4002').boss(MAGMAREX, 0, 180, hp(MAGMAREX) * 0.3 / 180).hidden(180, 181)
               .show_result(181, 188, 13, 73419, 180))
    # g1) week 4003's middle fight, its title read as the right boss (Alpha Bouldus) from 8 s on: not the
    #     partner, so three reads end the fight as 'different boss', as before
    g1 = Script('4003').boss(BLAZEN, 0, 60, hp(BLAZEN) * 0.4 / 60).hidden(60, 61)
    g1.title(8, 61, f"Lv. {LEVEL} Alpha Bouldus"); S['g1'] = g1
    # g2) the same on week 4003's left boss alone
    g2 = Script('4003').boss(SHERRO, 0, 60, hp(SHERRO) * 0.4 / 60).hidden(60, 61)
    g2.title(8, 61, f"Lv. {LEVEL} Alpha Bouldus"); S['g2'] = g2
    # h) the meter joins at the second boss (started mid-run, the first boss never seen): its full bar at x21 with 2:20
    #    on the clock is no time limit; killed at 170 s, 40 stars: cleared under the run's 3:00
    S['h'] = (Script('4003').boss(TUCKING, 40, 170, hp(TUCKING) / 130, kill=True).dead(TUCKING, 170, 172.8)
              .hidden(172.8, 173).show_result(173, 180, 40, 47100, 170))
    # i) week 4004: the first boss down at 30 s; the second hit for 3.2 s before its bar shows, so the bar first shows
    #    x14 (no top bar for the bar's own sign): its name, read within 2 s, hands over. The team wipes at 120 s
    h1, h2 = hp(GLAMEEP2), hp(JABSTER2); d2 = 0.02 * h2 / 3.2
    S['i'] = (Script('4004').boss(GLAMEEP2, 0, 30, h1 / 30, kill=True).dead(GLAMEEP2, 30, 32.8).hidden(32.8, 36, JABSTER2, d2)
              .boss(JABSTER2, 36, 120, d2).hidden(120, 121).show_result(121, 128, 30, 30500, 120))
    return S

# ---------------------------------------------------------------- checks
fails = []
def check(scn, what, ok, detail=''):
    if not ok: fails.append(f'{scn}: {what}' + (f' ({detail})' if detail else ''))

def statuses(r, prefix): return [(t, d) for t, k, d in r.outs if k == 'status' and d.startswith(prefix)]

def common(r, phases, limit, cleared, secs):
    """One run, its bosses in order, one clock, the limit from the first phase, no end between the bosses."""
    n = r.name; s = r.script
    starts, overs, ends = statuses(r, 'fight started'), statuses(r, 'fight over'), [x for x in r.outs if x[1] == 'fight_end']
    nexts = statuses(r, 'next boss')
    check(n, 'one fight started', len(starts) == 1, starts)
    check(n, "one 'fight over', at the run's end", len(overs) == 1 and overs[0][0] >= s.parts[-1][2] - 5, overs)
    check(n, 'the overlay got its final numbers at most once', len(ends) <= 1, ends)
    check(n, 'one Fight object for the run', len(r.fights) == 1, len(r.fights))
    f = r.fights[0]
    got = [p['row']['puppetId'] for p in f.phases]
    check(n, 'bosses in order', got == phases, got)
    check(n, "a 'next boss' status per hand-off", len(nexts) == len(phases) - 1, nexts)
    if nexts and overs: check(n, "no 'fight over' between the bosses", all(t < overs[0][0] for t, _ in nexts))
    check(n, 'every hand-off on the frame loop', all(c[3] for c in r.calls['next_boss']), r.calls['next_boss'])
    # one clock: the run's elapsed time never goes back and follows the timeline's own
    el = [(t, e) for t, fi, e, d, read in r.frames if fi is f and read]     # looks that read the bar
    # (the clock reads whole seconds, so a reading can set it back under a second: that is the Timer, not a reset)
    back = [(t0, e0, t1, e1) for (t0, e0), (t1, e1) in zip(el, el[1:]) if t0 > 3 and e1 < e0 - 1.1]
    check(n, 'the clock never goes back', not back, back[:3])
    for at, pid, why, _ in r.calls['next_boss']:
        near = [(t, e) for t, e in el if abs(t - at) <= 2]
        check(n, 'the clock goes on through the hand-off', near and max(abs(e - t) for t, e in near) <= 1.5, (at, near[:2], near[-2:]))
    off = [(t, e) for t, e in el if e > 1 and abs(e - t) > 1.5 and t < secs]
    check(n, "the run's time follows the game's clock", not off, off[:3])
    check(n, 'time limit from the first boss', f.limit == limit, f.limit)
    summ = next(iter(r.summaries.values()), None)
    check(n, 'a run summary', summ is not None)
    if summ:
        res = summ.get('result') or {}
        check(n, 'the time limit in the summary', (summ.get('meter') or {}).get('time_limit_s') == limit, summ.get('meter'))
        check(n, 'cleared as the result screen calls it', res.get('cleared') == cleared, res)
        check(n, "the result screen's time", res.get('time_s') == secs, res.get('time_s'))
    return f, summ

def damage_checks(r, f, defeated):
    """The first boss counts its full max HP once down; the run's damage is that plus the current boss's."""
    n = r.name; s = r.script
    for i, p in enumerate(f.phases):
        pid = p['row']['puppetId']; truth = s.lost(pid, s.end)
        check(n, f'boss {i + 1} max HP', p['max_hp'] == hp(pid), (p['max_hp'], hp(pid)))
        check(n, f'boss {i + 1} defeated', p['defeated'] == defeated[i], p)
        if i < len(f.phases) - 1:
            check(n, f'boss {i + 1} counts its max HP', p['damage'] == p['max_hp'], (p['damage'], p['max_hp']))
        else:
            # the last reading before the bar went: within 0.5% of the boss's max HP of what the timeline dealt
            check(n, f'boss {i + 1} damage', abs(p['damage'] - truth) <= 0.005 * p['max_hp'] or (defeated[i] and p['damage'] == p['max_hp']),
                  (p['damage'], round(truth)))
    total = sum(p['damage'] for p in f.phases)
    check(n, 'run damage = the bosses down + the current one', f.damage == total, (f.damage, total))
    check(n, "run max HP = the bosses' so far", f.max_hp == sum(p['max_hp'] for p in f.phases), f.max_hp)

def two_boss_extra(r, f, summ, first):
    n = r.name
    m = summ.get('meter') or {}
    bosses = m.get('bosses')
    check(n, 'the summary lists both bosses', bosses and [b['id'] for b in bosses] == [p['row']['puppetId'] for p in f.phases], bosses)
    key = next(iter(r.bests), '')
    check(n, "the best kept under the stage's bossId (the first boss)", key.endswith('|' + first) and len(r.bests) == 1, list(r.bests))
    res = summ.get('result') or {}
    check(n, 'the 40-star cap', res.get('star_cap') == 40, res.get('star_cap'))

def checks(R_):
    a, b, c, d, e, f1, f2, g1, g2, h, i = (R_.get(k) for k in ('a', 'b', 'c', 'd', 'e', 'f1', 'f2', 'g1', 'g2', 'h', 'i'))
    if a:
        f, s = common(a, [BLAZEN, TUCKING], 180, True, 150)
        damage_checks(a, f, [True, False])
        two_boss_extra(a, f, s, BLAZEN)
        t_in = next(p for p in a.script.parts if p[0] == 'boss' and p[1] == TUCKING)[2]
        nb = a.calls['next_boss']
        check('a', 'handed over by the bar within 1 s of the second boss showing', nb and 'bar came back' in nb[0][2] and nb[0][0] - t_in <= 1.0, nb)
        check('a', "the result screen's score as read", s['result']['score'] == 52318, s['result'])
    if b:
        f, s = common(b, [BLAZEN, TUCKING], 180, False, 180)
        damage_checks(b, f, [True, False])
        two_boss_extra(b, f, s, BLAZEN)
        check('b', 'the bar never vanished', not any(p[0] == 'hidden' for p in b.script.parts if p[3] <= 180))
        # the title read cut before the hand-off named the first boss and came after it: no switch vote
        late = [c for c in b.identify_after if 'Blazen' in (c[1] or '')]
        check('b', 'a title read from before the hand-off came after it', bool(late), b.calls['identify'][-4:])
        check('b', '...and cast no vote to end the run', all(not c[3] for c in late), late)
        check('b', 'two-boss score kept as read (no 7->1 repair)', s['result']['score'] == 37250, s['result'])
        check('b', 'the one-boss rule would have repaired it', live.interlink_score(37250, 31, 40) == 31250)
    if c:
        f, s = common(c, [BLAZEN, TUCKING], 240, True, 230)
        damage_checks(c, f, [True, False])
        two_boss_extra(c, f, s, BLAZEN)
    if d:
        f, s = common(d, [GLAMEEP2], 180, False, 180)
        damage_checks(d, f, [False])
        check('d', 'no hand-off', not d.calls['next_boss'], d.calls['next_boss'])
        check('d', 'no bosses list for a one-boss run', 'bosses' not in (s.get('meter') or {}), s.get('meter'))
        key = next(iter(d.bests), '')
        check('d', "the best kept under the stage's bossId", key.endswith('|' + GLAMEEP2), list(d.bests))
    if e:
        f, s = common(e, [GRIZBO2, JEBURST2], 180, False, 120)
        damage_checks(e, f, [True, False])
        check('e', 'no Interlink scene from the game log (so not sent)', s.get('interlink_scene') is False, s.get('interlink_scene'))
        gap = next(p for p in e.script.parts if p[0] == 'hidden')
        check('e', 'the bar was gone longer than the 20 s grace', gap[0] == 'hidden' and gap[3] - gap[2] > live.BAR_GONE_GRACE, gap)
    for r, pid, kill, secs in ((f1, SHERRO, True, 100), (f2, MAGMAREX, False, 180)):
        if not r: continue
        f, s = common(r, [pid], 180, kill, secs)
        damage_checks(r, f, [False])                # ended on the result screen: the meter's last reading
        check(r.name, 'no partner', f.partner is None)
        check(r.name, 'no bosses list', 'bosses' not in (s.get('meter') or {}))
        key = next(iter(r.bests), '')
        check(r.name, 'the best under its own id', key.endswith('|' + pid), list(r.bests))
    if f2:
        s = next(iter(f2.summaries.values()))
        check('f2', 'one-boss score still repaired (73419 -> 13419)', s['result']['score'] == 13419, s['result'])
    if h:
        f, s = common(h, [TUCKING], None, True, 170)
        damage_checks(h, f, [False])
        check('h', 'not tracked from the run start', f.started_at_top is False, f.started_at_top)
        key = next(iter(h.bests), '')
        check('h', "the best kept under the stage's bossId (the first boss)", key.endswith('|' + BLAZEN), list(h.bests))
    if i:
        f, s = common(i, [GLAMEEP2, JABSTER2], 180, False, 120)
        damage_checks(i, f, [True, False])
        two_boss_extra(i, f, s, GLAMEEP2)
        t_in = next(p for p in i.script.parts if p[0] == 'boss' and p[1] == JABSTER2)[2]
        nb = i.calls['next_boss']
        check('i', 'handed over by the name within 2.5 s of the bar showing', nb and 'name read' in nb[0][2] and nb[0][0] - t_in <= 2.5, nb)
    for r, first in ((g1, BLAZEN), (g2, SHERRO)):
        if not r: continue
        starts, overs = statuses(r, 'fight started'), statuses(r, 'fight over')
        check(r.name, 'a fight started, then a new one for the other boss', [d_.split(':')[1].split(' Lv')[0].strip() for _, d_ in starts]
              == [ROW[first]['bossTitle_en'], 'Alpha Bouldus'], starts)
        check(r.name, "ended as 'different boss'", overs and 'different boss' in overs[0][1], overs)
        check(r.name, 'no hand-off', not r.calls['next_boss'], r.calls['next_boss'])

# ---------------------------------------------------------------- run
def trace(r):
    """What the meter put out, for comparing two checkouts (wall times relative to the run, new keys left out)."""
    keep = lambda d: {k: v for k, v in d.items() if k not in ('boss_hp', 'boss', 'screen', 'bar', 'break')} if isinstance(d, dict) else d
    outs = [(t, k, keep(d)) for t, k, d in r.outs if k != 'fight' or int(t * 30) % 15 == 0]
    summ = json.loads(json.dumps(list(r.summaries.values())))        # a copy: the checks read the summaries after
    for s in summ:
        for k in ('bosses', 'csv'): (s.get('meter') or {}).pop(k, None)
    return {'outs': outs, 'csv': [[','.join(l.split(',')[1:]) for l in rows] for rows in r.csvs.values()], 'summaries': summ,
            'bests': r.bests}

def main():
    if not ARGS.v: logging.getLogger('meter').addHandler(logging.NullHandler())
    else:
        h = logging.StreamHandler(sys.stdout); h.setFormatter(logging.Formatter('%(message)s'))
        logging.getLogger('meter').addHandler(h); logging.getLogger('meter').setLevel(logging.INFO)
    stub_readers()
    every = scenarios()
    names = ARGS.only.split(',') if ARGS.only else list(every)
    results = {}
    for name in names:
        t0 = REAL_TIME()
        r = run(name, every[name])
        r.identify_after = [c for c in r.calls['identify'] if r.calls['next_boss'] and c[0] >= r.calls['next_boss'][0][0]]
        results[name] = r
        f = r.fights[0] if r.fights else None
        phases = getattr(f, 'phases', None)
        print(f"{name}: {len(r.fights)} fight(s), bosses {[p['row']['bossTitle_en'] for p in phases] if phases else (f and f.title)}, "
              f"damage {f and f.damage:,} of {f and f.max_hp:,}, {f and f.elapsed:.1f}s, limit {f and f.limit}, "
              f"hand-offs {[(c[0], c[2]) for c in r.calls['next_boss']]} ({REAL_TIME() - t0:.1f} s)")
    if ARGS.trace:
        with open(ARGS.trace, 'w', encoding='utf-8') as fh: json.dump({k: trace(v) for k, v in results.items()}, fh, indent=1, default=str)
    if not ARGS.no_checks: checks(results)
    shutil.rmtree(TMP, ignore_errors=True)
    for x in fails: print('FAIL', x)
    print(f'{len(names)} scenarios, {len(fails)} failures')
    sys.exit(1 if fails else 0)

if __name__ == '__main__':
    main()
