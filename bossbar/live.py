"""Aniimo live boss DPS meter (team damage from the boss HP bar).

Watches the game window through the aLogs app's shared capture (on the graphics card, wincapture.py), reads the
boss bar, and shows team damage and DPS in a small always-on-top overlay. It also follows the
game's own Player.log for scene changes, which mark exactly when a boss attempt starts and ends.
It only looks at screen pixels and the game's text log; it never touches the game's memory or files.

Usage:
    python live.py [--interval 0.05] [--no-overlay] [--hide-from-capture] [--no-debug-frames] [--replay shots...]
"""
import argparse, collections, csv, ctypes, difflib, json, math, logging, logging.handlers, os, platform, queue, re, sys, threading, time
from PIL import Image, ImageChops, ImageDraw, ImageFont, ImageOps, ImageStat
import reader as R
import pcstats
import wincapture as wc
import xn_digits as X
from inputblock import InputBlock
import controller as CT
import eggheist as E
import hangulnames as K
import submit as S
from presence import Presence

import settings as C
HERE = C.DATA                     # everything the meter writes (logs, records, positions)
NOTICES = C.NOTICES
PLAYER_LOG = os.path.join(os.environ.get('USERPROFILE', ''), 'AppData', 'LocalLow', 'Aniimo', 'Aniimo', 'Player.log')
METER_LOG = os.path.join(HERE, 'logs', 'meter.log')
BAR_GONE_GRACE = 20     # seconds a fight waits for its bar to come back (ultimates and effects can hide it)
# The boss HP bar's colour along its track (the same on every boss, sampled from 4K frames): the word HP on the HP
# label takes the colour where the HP ends, light orange near full and as red as the bar when it's low
HP_GRADIENT = ((0.0, (248, 108, 108)), (0.25, (250, 120, 104)), (0.4, (249, 131, 98)), (0.55, (251, 144, 94)),
               (0.7, (254, 157, 91)), (0.85, (255, 167, 88)), (1.0, (255, 176, 85)))

def hp_colour(frac):
    """The HP bar's colour at this share of the track (0 the left end, 1 full)."""
    frac = min(1.0, max(0.0, frac or 0.0))
    for (a, ca), (b, cb) in zip(HP_GRADIENT, HP_GRADIENT[1:]):
        if frac <= b:
            t = (frac - a) / (b - a)
            return tuple(round(x + (y - x) * t) for x, y in zip(ca, cb))
    return HP_GRADIENT[-1][1]
BAR_GONE_S = 3          # seconds a locked bar's spot must read like no HP bar at all before the fight ends
KILL_SOUND_WINDOW = (2.5, 6.0)  # the boss kill sound counts when it started this many seconds before the countdown
KILL_TO_COUNTDOWN = 3.0 # seconds from the killing blow to the game's Catch Countdown (the kill-cam: no HUD, slow
                        # motion, speed lines; 3.0-3.1 s on 11 Alpha kills, measured frame by frame)
TOP_PART = 0.42         # share of the frame captured while a bar is locked (bar, title, xN, timer are all up there)
FIGHT_INTERVAL = 1 / 30 # seconds between frames during a fight
DAMAGE_NUMBERS = False  # the damage-number recorder (dmgnumbers.py, capture.damage_numbers): paused, off whatever the setting
INK_MIN = 40            # black pixels a prepared white-text crop needs before it is worth reading (screen_crops)
EGG_BLACK = 0.12        # s of black screen that count as Egg Heist's fade to its end screen (egg_fade)
AWAY_INTERVAL = 0.25    # between fights while the game isn't the active window (a running fight keeps 1/30 s)
HOMELAND_SCENE = 550    # a homeland's scene id ('100-<player id>-550'): nothing is captured there for now
IDLE_INTERVAL = 0.25    # out in the world with no boss bar and nothing to watch: 4 looks a second (a player's lag
                        # report, B4: 8-13 whole 4K window copies a second while just standing at home)
CONFIRM_GAP = 0.09      # a reading that confirms an earlier one (a big drop, a refill, a counter vote) must come at
                        # least this long after it, so a one-off effect lasting a few fast frames can't confirm itself
LEVEL_CONFIRM = 3       # s an Interlink level other than the season's must be read alone before a fight starts on it
XN_STALE = 40           # s a boss rush counter read keeps bounding refills (the late bars take 15-25 s each)
SETTINGS = os.path.join(HERE, 'overlay_positions.json')

# ---------------------------------------------------------------- diagnostic log
log = logging.getLogger('meter')
def setup_log():
    """logs/meter.log: everything the meter does and sees, for bug reports. Capped at 4 x 5 MB."""
    os.makedirs(os.path.dirname(METER_LOG), exist_ok=True)
    h = logging.handlers.RotatingFileHandler(METER_LOG, maxBytes=5 << 20, backupCount=3, encoding='utf-8')
    h.setFormatter(logging.Formatter('%(asctime)s.%(msecs)03d %(levelname)-5s %(message)s', '%Y-%m-%d %H:%M:%S'))
    log.addHandler(h); log.setLevel(logging.DEBUG)
    log.info('=' * 30 + ' meter started ' + '=' * 30)
    log.info(f'python {sys.version.split()[0]} | {platform.platform()} | args {" ".join(sys.argv[1:]) or "(none)"}')
    try:
        u = ctypes.windll.user32
        log.info(f'screen {u.GetSystemMetrics(0)}x{u.GetSystemMetrics(1)}, monitors {u.GetSystemMetrics(80)}')
    except Exception: pass

# ---------------------------------------------------------------- game log
# Your own UID: the callerId of the game's service calls and your profile files. A personal space id
# ('100-<uid>-<scene>') is only a fallback: it's the owner's, and visiting someone's home put their UID on your runs
# (30 Oct, 100-302000245537-550: a Glameep run was filed under that player).
RX_UID = re.compile(rb'callerId=(\d{6,20})|/prod/ClientInfo/(\d{6,20})_client_info_bin|/prod/pet_jewelry/(\d{6,20})')
RX_SPACE_UID = re.compile(rb'createVoxelScene new>>>id:\d+-(\d{6,20})-\d+')

def uid_in(raw):
    """Your UID from one game-log line (bytes), or None."""
    m = RX_UID.search(raw)
    return next((g for g in m.groups() if g), None) if m else None

class LogWatcher(threading.Thread):
    """Follows the end of Player.log and reports scene changes. Never reads the old (huge) part of the file."""
    RX_ENTER = re.compile(r'createVoxelScene new>>>id:(\S+)')
    RX_LEAVE = re.compile(r'clearSpace is_destroy:true id:(\S+)')
    RX_TIME = re.compile(r'\[(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d\.\d+)\]')
    def __init__(self, path, events):
        super().__init__(daemon=True); self.path, self.events, self.stop = path, events, False
        self.uid = None
        self.current = None                          # the space the game entered last
        self.delays = collections.deque(maxlen=20)   # seconds between the game logging a line and us seeing it

    def _delay(self, line):
        m = self.RX_TIME.search(line)
        if not m: return None
        try:
            t = time.mktime(time.strptime(m.group(1).split('.')[0], '%Y-%m-%d %H:%M:%S')) + float('0.' + m.group(1).split('.')[1])
        except ValueError: return None
        d = time.time() - t
        if -5 < d < 600: self.delays.append(d)
        return d
    def look_back(self, size, span=16 << 20):
        """Started in the middle of a visit: the last space entered, and whether it is an Interlink one (the game
        writes the Interlink scene line only once, right after entering)."""
        try:
            with open(self.path, 'rb') as f:
                f.seek(max(0, size - span)); tail = f.read(span)
        except OSError: return
        rush = set(); sim = set(); last = None
        for raw in tail.split(b'\n'):
            m = self.RX_ENTER.search(raw.decode('utf-8', 'replace')) if b'createVoxelScene' in raw else None
            if m: last = m.group(1)
            elif b'fb_bossrush' in raw and last: rush.add(last)
            elif b'fb_roguelike' in raw and last: sim.add(last)
        if last:
            self.current = last
            self.events.put(('enter', last, None))
            if last in rush: self.events.put(('bossrush', last, None))
            if last in sim: self.events.put(('holosim', last, None))

    def run(self):
        pos = None; buf = b''
        while not self.stop:
            try:
                size = os.path.getsize(self.path)
                if pos is None: self.look_back(size)
                if pos is None or size < pos: pos = size; buf = b''          # start at the end / log was recreated
                if size > pos:
                    with open(self.path, 'rb') as f:
                        f.seek(pos); chunk = f.read(min(size - pos, 64 << 20)); pos += len(chunk)
                    lines = (buf + chunk).split(b'\n'); buf = lines.pop()
                    for raw in lines:
                        if b'callerId=' in raw or b'/prod/' in raw:             # service calls and login downloads
                            uid = uid_in(raw)
                            if uid and uid != self.uid:
                                self.uid = uid; self.events.put(('uid', uid.decode(), None))
                            if b'createVoxelScene' not in raw: continue
                        if (b'createVoxelScene' not in raw and b'clearSpace' not in raw and b'bossrush' not in raw
                                and b'fb_roguelike' not in raw):
                            if b'][info]' in raw and len(self.delays) < 20: self._delay(raw.decode('utf-8', 'replace'))
                            continue
                        line = raw.decode('utf-8', 'replace'); d = self._delay(line)
                        m = self.RX_ENTER.search(line)
                        if m: self.current = m.group(1); self.events.put(('enter', m.group(1), d)); continue
                        m = self.RX_LEAVE.search(line)
                        if m: self.events.put(('leave', m.group(1), d)); continue
                        if 'fb_bossrush' in line: self.events.put(('bossrush', self.current, d))
                        elif 'fb_roguelike' in line: self.events.put(('holosim', self.current, d))   # Holo-Battle Sim
            except OSError: pass
            time.sleep(0.25)

class CpuLoad:
    """CPU used since the last sample, as % of the whole PC (all cores = 100): the meter, the game and everything,
    from GetProcessTimes / GetSystemTimes. For the once-a-minute "load:" line, so a lag report shows what each
    took; None where Windows doesn't answer."""
    def __init__(self):
        import ctypes.wintypes as W
        k = ctypes.windll.kernel32; self.last = None
        k.GetCurrentProcess.restype = k.OpenProcess.restype = ctypes.c_void_p          # 64-bit handles
        k.OpenProcess.argtypes = [W.DWORD, W.BOOL, W.DWORD]; k.CloseHandle.argtypes = [ctypes.c_void_p]
        k.GetProcessTimes.argtypes = [ctypes.c_void_p] + [ctypes.POINTER(W.FILETIME)] * 4
    @staticmethod
    def _ft(ft): return (ft.dwHighDateTime << 32 | ft.dwLowDateTime) / 1e7          # 100 ns units -> s
    def _proc(self, handle):
        k = ctypes.windll.kernel32; ft = [ctypes.wintypes.FILETIME() for _ in range(4)]
        if not k.GetProcessTimes(handle, *[ctypes.byref(f) for f in ft]): return None
        return self._ft(ft[2]) + self._ft(ft[3])                                        # kernel + user
    def _pid(self, pid):
        if not pid: return None
        k = ctypes.windll.kernel32; h = k.OpenProcess(0x1000, False, pid)                # QUERY_LIMITED_INFORMATION
        if not h: return None
        try: return self._proc(h)
        finally: k.CloseHandle(h)
    def sample(self, game_pid, app_pid=0):
        k = ctypes.windll.kernel32; idle, kern, user = (ctypes.wintypes.FILETIME() for _ in range(3))
        if not k.GetSystemTimes(ctypes.byref(idle), ctypes.byref(kern), ctypes.byref(user)): return None
        total = self._ft(kern) + self._ft(user); busy = total - self._ft(idle)          # kernel time includes idle
        me = self._proc(k.GetCurrentProcess())
        game, app = self._pid(game_pid), self._pid(app_pid)
        now, self.last, last = (total, busy, me, game, game_pid, app), (total, busy, me, game, game_pid, app), self.last
        if not last or now[0] <= last[0]: return None
        span = now[0] - last[0]
        pct = lambda a, b: round(100 * (a - b) / span, 1) if a is not None and b is not None else None
        return {'pc': pct(now[1], last[1]), 'meter': pct(now[2], last[2]),
                'game': pct(now[3], last[3]) if now[4] == last[4] else None, 'app': pct(now[5], last[5])}

def scene_of(space_id):
    """'100-<playerId>-<sceneId>' -> sceneId (homeland 550, open world 3000 ...); instances return None."""
    m = re.match(r'^\d+-\d+-(\d+)$', space_id or ''); return int(m.group(1)) if m else None

# Labels exactly as the game writes them (its English text table): a read is snapped to the closest one, so small
# text at 720p ("Tot al Da nn age") still records the game's wording.
RESULT_STATS = ('Total Damage Dealt', 'Total BREAK Damage', 'Targets Defeated', 'Most Healing', 'Most Damage Taken',
                'Most Hits Taken', 'Most Support Provided',
                'Best Strike Damage')     # the Interlink result screen's per-player highlight
# 'Best Strike Damage' (text id 1277896829) was left out: it stayed as read, so a slot with only that label read dropped
# out (321 slot reads of 31 Oct-1 Nov carry it, all with a value read so far); of the 2228
# logged label reads, only it and 2 misreads of it snap differently
EGG_MODES = ('Egg Heist Team Mode', 'Egg Heist')

def canon(text, choices, cutoff=0.6):
    """The game label `text` most likely is (letters only, case-insensitive), or `text` as read if none is close."""
    norm = lambda s: re.sub(r'[^a-z]', '', (s or '').lower())
    t = norm(text)
    if not t: return text
    best = max(choices, key=lambda c: difflib.SequenceMatcher(None, t, norm(c)).ratio())
    return best if difflib.SequenceMatcher(None, t, norm(best)).ratio() >= cutoff else text

CLEAR_STARS = 20       # an Interlink clear: the full rating, 20 on the left and right bosses (sys_config_data.BossRushMaxStarSideBoss)
CENTER_STARS = 40      # and 40 on the center boss (x BossRushStarMultiplierMidBoss, the HUD's cap: BossRushInfoUIComponent.setBossRushStar);
                       # a 26-star center run that ended at 95 s with most of its HP left was no clear

STAR_CAPS = os.path.join(HERE, 'logs', 'star_caps.json')   # the caps the Interlink screen last showed: {'side', 'center', 'cycle'}

def star_caps():
    """The side and center bosses' full ratings. They went from 15/30 to 20/40 with the Whisperwake update and may move
    again, so they come from the game itself: its config, read with each patch (game_cycles.json 'rules':
    BossRushMaxStarSideBoss x BossRushStarMultiplierMidBoss), else what the Interlink screen last showed ('14/20',
    '36/40'), else the built-in 20/40."""
    for path, keys in ((os.path.join(HERE, 'game_cycles.json'), ('rules', 'starSide', 'starCenter')), (STAR_CAPS, (None, 'side', 'center'))):
        try:
            with open(path, encoding='utf-8') as f: c = json.load(f)
            c = c[keys[0]] if keys[0] else c
            return int(c[keys[1]]), int(c[keys[2]])
        except (OSError, ValueError, KeyError, TypeError): continue
    return CLEAR_STARS, CENTER_STARS

def clear_stars(puppet_id):
    """The full rating of an Interlink boss: the center boss's cap for this week's center boss, the side cap otherwise.
    The cap is the place's, not the boss's (BossRushUtils.getCurBossRushPlace, line 288; BossRushInfoUIComponent.
    setBossRushStar, line 187), so both bosses of a two-boss middle fight (weeks 4003-4005: bossrush_guanka_data
    bossId + bossId_2) get the center's: cycle_slot counts both."""
    side, center = star_caps()
    return center if puppet_id is not None and R.cycle_slot(R.active_cycle_id(), puppet_id)[0] == 'middle' else side

def interlink_score(score, stars, cap=None, two_boss=False):
    """An Interlink score checked against its stars. It is not stars x 1,000: each grade a boss passes is worth its
    startScore + hpScore (bossrush_grade_data: 500 + 500 a star grade, 2,500 + 2,500 the kill, "Eliminate"), the
    center boss's stars count double (sys_config_data.BossRushStarMultiplierMidBoss, BossRushInfoUIComponent's star
    cap) and its score too (real runs only: no game file says so), and the server adds them up. No client Lua reads
    those columns, so that sum is inferred; it fits every real run read right. Below the cap the score is in its
    stars' thousand (12,058 is 12 stars); at the cap it is anything above that, as full stars keep scoring until the
    clock runs out and a kill adds more (21,519 on a 20-star boss is 20). A score whose thousands don't match had a 1
    read as a 7 (OCR: 17164 as 77764, 13419 as 73419): each 7 is tried as a 1, and the one reading that matches is
    used. When more than one does, the score is left unread (None) rather than sent wrong. The stars are never changed
    from the score. two_boss: a two-boss middle fight's run (R.stage_of gives its boss an order; weeks 4003-4005). How
    the server builds its one score from two bosses is unknown until a live run shows it (no client Lua reads those
    score columns, and kills add bonus score), so a score that doesn't fit its stars is kept as read: the reading
    may well be right, and only the font-sure read is trusted there."""
    if score is None or stars is None: return score
    fits = lambda s: s // 1000 == stars or (cap is not None and stars == cap and s // 1000 > stars)
    if fits(score): return score
    if two_boss:
        log.info(f"score {score} kept as read: {stars} stars, a two-boss fight (how its score is built isn't known)"); return score
    digits = str(score); sevens = [i for i, c in enumerate(digits) if c == '7']
    found = set()
    for mask in range(1, 1 << len(sevens)):
        d = list(digits)
        for j, i in enumerate(sevens):
            if mask >> j & 1: d[i] = '1'
        if fits(int(''.join(d))): found.add(int(''.join(d)))
    if len(found) == 1:
        fixed = found.pop(); log.info(f'score {score} read as {fixed}: {stars} stars (a 1 read as a 7)'); return fixed
    if found:                                          # more than one fits (77764: 17164 or 17764): not sent as read
        log.info(f'score {score} left unread: {stars} stars, and it could be any of {sorted(found)}'); return None
    return score

def first_num(s):
    """The number in an OCR read of tiled short text ('60 60 60' -> 60): the one most tiles agree on, so one misread
    tile doesn't win by coming first ('6 36 36' was read as 6 stars)."""
    nums = re.findall(r'\d+', (s or '').replace('O', '0'))
    return int(max(nums, key=lambda n: (nums.count(n), -nums.index(n)))) if nums else None

# Holo-Battle Sim's stage line, as the game's rogue_difficulty_data names it (interfaceName). Season 1: "Training of
# Water - Hard" (Easy, "Common" for Normal, Hard) and "Trial of Fire - Challenge". Season 2 on: "Earth Training -
# Easy" ... "- Trial". Each season brings new elements (Grass/Water/Fire, then Earth/Lightning/Dark), so any word is
# taken as the element. Between the stage and its difficulty: " - ", which OCR sometimes reads with a stray letter.
SIM_DIFF = r'(Easy|Common|Normal|Hard|Challenge|Trial)(?![A-Za-z]{2})'   # ('Hardk': a stray letter after it)
SIM_STAGE_OF = re.compile(r'\b(Training|Trial|Crucible)\s*of\s*([A-Za-z]{3,12}).{0,6}?' + SIM_DIFF, re.I)
SIM_STAGE = re.compile(r'\b([A-Za-z]{3,12})\s*(Training|Trial).{0,6}?' + SIM_DIFF, re.I)

def parse_sim(text):
    """Holo-Battle Sim's Victory line "Battle time 00:08:22 Earth Training - Hard" (or Season 1's "Training of Water -
    Hard") -> {'time_s': 502, 'trial': 'Earth Training', 'element': 'Earth', 'difficulty': 'Hard'}, or None unless the
    time and the stage both read. The stage name is kept as the game shows it."""
    t = re.search(r'(\d{1,2})\s*[:.]\s*(\d{2})\s*[:.]\s*(\d{2})', text or '')
    if not t or int(t.group(2)) > 59 or int(t.group(3)) > 59: return None
    rest = (text or '')[t.end():]
    s = SIM_STAGE_OF.search(rest)
    if s: kind, element, difficulty = (g.capitalize() for g in s.groups()); trial = f'{kind} of {element}'
    else:
        s = SIM_STAGE.search(rest)
        if not s or s.group(1).lower() in ('time', 'battle'): return None
        element, kind, difficulty = (g.capitalize() for g in s.groups()); trial = f'{element} {kind}'
    return {'time_s': int(t.group(1)) * 3600 + int(t.group(2)) * 60 + int(t.group(3)),
            'trial': trial, 'element': element, 'difficulty': difficulty}

# Holo-Battle Sim's Victory screen (UI_Pb_Tower_Settlement), as fractions of the 16:9 UI area: the team on the left
# (the MVP's big card, then four smaller ones, each with "Lv. 60" over the name), the MVP's damage / damage taken /
# healing shares beside its card, the coins, and the Holo-gizmo series row with a count badge under each icon.
SIM_CARDS = ((0.135, 0.385, 0.26, 0.45), (0.13, 0.555, 0.27, 0.61), (0.268, 0.575, 0.385, 0.63),
             (0.13, 0.745, 0.25, 0.80), (0.248, 0.745, 0.37, 0.80))
SIM_SHARES = (0.352, 0.30, 0.40, 0.40)
SIM_COINS = (0.795, 0.285, 0.84, 0.325)
SIM_GIZMO_X, SIM_GIZMO_PITCH, SIM_GIZMO_Y = 0.464, 0.0588, (0.5795, 0.5985)
SIM_GIZMOS = 7
# Without a "Rewards Earned" row (no first clear: 3 Nov, Lightning Training - Trial) the right side's contents sit this
# much lower (fractions of H): the coins, the Data Collection pictures, the gizmo row and its counts. The team and
# shares on the left don't move. Both heights are read; the one that reads is kept (parse_sim_team 'low').
SIM_LOW = 0.0417

def sim_badges(im, u, ux, H, dy=0.0):
    """The digits on the Holo-gizmo badges (dark on light grey, too small for Windows OCR on their own), pasted in
    one line after a drawn label: 'n: / 6 / 6 / 2 / 2 / 4 / 5'. Empty slots (an X) give nothing."""
    from PIL import ImageDraw, ImageFont
    try: font = ImageFont.truetype('arialbd.ttf', 60)
    except OSError: font = ImageFont.load_default()
    k = u / 2000
    out = Image.new('L', (140 + SIM_GIZMOS * 110, 120), 255); ImageDraw.Draw(out).text((10, 25), 'n:', fill=0, font=font)
    for i in range(SIM_GIZMOS):
        cx = SIM_GIZMO_X + SIM_GIZMO_PITCH * i
        c = im.crop((int(ux + u * (cx - 0.0075)), int(H * (SIM_GIZMO_Y[0] + dy)), int(ux + u * (cx + 0.0075)), int(H * (SIM_GIZMO_Y[1] + dy)))).convert('L')
        c = c.resize((max(1, int(c.width * 4 / k)), max(1, int(c.height * 4 / k))), Image.LANCZOS).point(lambda v: 0 if v < 120 else 255)
        c.thumbnail((90, 90)); out.paste(c, (120 + i * 110, 15))
    return out

def sim_team_crops(im, u, ux, H):
    """The Victory screen's team, shares, coins and gizmo counts, prepared for one OCR call."""
    k = u / 2000; b = lambda x0, y0, x1, y1: (ux + u * x0, H * y0, ux + u * x1, H * y1)
    crops = {}
    for i, box in enumerate(SIM_CARDS):
        crops[f'c{i}w'] = R.prep_white(im, b(*box), k); crops[f'c{i}b'] = R.prep_bright(im, b(*box), k)
    crops['share'] = R.prep_white(im, b(*SIM_SHARES), k)
    crops['coins'] = R.prep_bright(im, b(*SIM_COINS), k)
    crops['gizmos'] = sim_badges(im, u, ux, H)
    x0, y0, x1, y1 = SIM_COINS                                    # ...and lower, when there's no Rewards Earned row
    crops['coinsL'] = R.prep_bright(im, b(x0, y0 + SIM_LOW, x1, y1 + SIM_LOW), k)
    crops['gizmosL'] = sim_badges(im, u, ux, H, SIM_LOW)
    return crops

def parse_sim_team(t, names=()):
    """What sim_team_crops() read -> {'team': [{'name', 'level', 'mvp'}], 'mvpShares': {'damage', 'taken', 'healing'},
    'gizmoCounts': [6, 6, 2, ...], 'coins': 8550}. A card's name is matched to the game's Aniimo names when given
    ('Ir?sahs' -> 'Irisalis'); a card that doesn't read is left out, so the team may be short but never wrong."""
    team = []
    for i in range(len(SIM_CARDS)):
        reads = []
        for v in (t.get(f'c{i}w'), t.get(f'c{i}b')):
            m = re.search(r'L\W?v\W*(\d{1,3})\W+(\S.{1,24})', v or '')
            if m and 1 <= int(m.group(1)) <= 100: reads.append((int(m.group(1)), m.group(2).strip()))
        if not reads: continue
        # the game name the card's reads come closest to (one read can be 'Ir?sahs', the other 'Irisalis'); else,
        # as the names come from the head portraits and a few Aniimo have none (Leafy), a read that is a plain name
        norm = lambda x: re.sub(r'[^a-z]', '', x.lower())
        score, name = max(((difflib.SequenceMatcher(None, norm(r), norm(n)).ratio(), n) for _, r in reads for n in names),
                          default=(0, None))
        if score < 0.7:
            plain = [r for _, r in reads if re.fullmatch(r"[A-Z][a-z]+(?:[ '-][A-Z][a-z]+)?", r)]
            if not plain: continue
            name = plain[0]
        team.append({'name': name, 'level': reads[0][0], 'mvp': i == 0})
    shares = []
    for part in (t.get('share') or '').split(' / '):      # OCR's line breaks (a '%' itself can read as '0/0')
        part = re.sub(r'\s*(%|[0oO]\s*/\s*[0oO]|[0oO]/[0oO])\s*$', '', part.strip())   # '%' reads as '0/0'
        n = re.findall(r'\d{1,3}', part)
        if n and int(n[-1]) <= 100: shares.append(int(n[-1]))
    def right(sfx):                                       # the coins and gizmo counts at one height
        coins = re.sub(r'[^\d]', '', t.get('coins' + sfx) or '')
        return ([int(n) for n in re.findall(r'\d{1,2}', (t.get('gizmos' + sfx) or '').split(':', 1)[-1])][:SIM_GIZMOS],
                int(coins) if coins and len(coins) <= 7 else None)
    (counts, coins), (countsL, coinsL) = right(''), right('L')
    low = (len(countsL), coinsL is not None) > (len(counts), coins is not None)
    if low: counts, coins = countsL, coinsL
    return {'team': team,
            'mvpShares': dict(zip(('damage', 'taken', 'healing'), shares)) if len(shares) == 3 else None,
            'gizmoCounts': counts, 'coins': coins, 'low': low}

def last_uid(path):
    """Your player id from the end of the game log (last 32 MB), the newest of: a personal-space id
    ('100-<uid>-<scene>'), the 'callerId=<uid>' of the game's service calls, or your profile files the game downloads
    right after login. Before login there is none: then there is no UID until the log watcher sees one. Never a
    remembered one - with characters on several servers it could be the wrong character."""
    try:
        with open(path, 'rb') as f:
            f.seek(max(0, os.path.getsize(path) - (32 << 20))); tail = f.read()
    except OSError: return None
    ids = [next(g for g in m.groups() if g) for m in RX_UID.finditer(tail)] or [m.group(1) for m in RX_SPACE_UID.finditer(tail)]
    return ids[-1].decode() if ids else None

def parse_battle_data(text):
    """Battle Data cards: OCR gives each card's 4 labels, then its 4 values in the same order
    (Average DPS, BREAK Value, Damage Taken, Heal Amount). A lone 'o'/'O' is a 0. Each card is read on its own,
    from one 'Average DPS' label to the next: a value the OCR missed (a blank Heal) used to shift every later card
    by one, so the next player's DPS became this one's heal. A card short of a value keeps the ones it has, in
    order, and the missing ones are None (never another player's): a card is kept even with one value, so the cards
    after it stay with their players. A downed player's card has no HP bar, and OCR reads its values on the label's
    line ('BREAK Value 2058717', '0 Damage Taken 21398': the 0 is the label's icon): a number ending a label is that
    label's value (1 Nov: the card was dropped and the next player's numbers moved onto this one). That card's lower
    lines can come after the next card's numbers, under their labels again ('O Damage Taken / O Heal Amount / 28605 /
    7737'): a label a card already had goes back to the nearest earlier card missing it, with the numbers after it."""
    keys = ('avg_dps', 'break', 'taken', 'heal'); cards = []
    def label(p):
        return 'break' if 'BREAK' in p else 'taken' if 'Damage' in p else 'heal' if 'Heal' in p else None
    for seg in re.split(r'[^/]*Average\s*D', text or '')[1:]:          # one segment per card, labels then values
        vals, seen, spill = [], set(), None                            # spill: [[key, value or None], ...]
        for part in (p.strip() for p in seg.split('/')):
            p = part.replace(',', '')
            if re.fullmatch(r'[oO]', p): p = '0'
            if re.fullmatch(r'\d+', p):
                if spill is not None:
                    gap = next((s for s in spill if s[1] is None), None)
                    if gap: gap[1] = int(p)
                else: vals.append(int(p))
            elif re.search(r'[A-Za-z]', p):
                key = label(p); m = re.search(r'\s(\d+)$', p)
                if key and key in seen and spill is None: spill = []
                if spill is not None:
                    if key: spill.append([key, int(m.group(1)) if m else None])
                    continue
                if key: seen.add(key)
                if m: vals.append(int(m.group(1)))
        card = dict(zip(keys, vals[:4] + [None] * (4 - len(vals[:4]))))
        cards.append(card)
        had = [sum(c[k] is not None for k in keys) for c in cards]       # counted before any spilled value lands
        for key, value in spill or []:      # to the shortest card missing it (the downed one), the nearest of those
            short = [i for i in range(len(cards) - 2, -1, -1) if cards[i][key] is None]
            owner = cards[min(short, key=lambda i: had[i])] if short else None
            if owner is not None and value is not None: owner[key] = value
    # a card with no number at all is a misread (the screen still opening, or half covered): no cards from this read,
    # rather than empty ones that would count as players
    if any(all(c[k] is None for k in keys) for c in cards): return []
    for card in cards:
        # a card one value short is missing its Heal: the lone '0' the OCR skips (every short card on 30 Oct, checked
        # against the proof pictures, was a Heal of 0)
        if sum(card[k] is not None for k in keys) == 3 and card['heal'] is None: card['heal'] = 0
    return cards

# ---------------------------------------------------------------- timer
OVERTIME_S = 60          # s: the Overtime offering's extra time (a 3:00 fight becomes 4:00; bossrush_buff_data[208].additionalTime)
MAX_CLOCK = 900          # s: readings above this are misreads; a run's own clock can start above --duration (Overtime: 4:00)

class Timer:
    """Countdown smoothed against the wall clock: OCR readings are only trusted when they agree with elapsed time.
    It never restarts a fight (combat effects make OCR misread it); fight boundaries come from the game log."""
    def __init__(self, duration): self.duration = duration; self.val = None; self.at = None; self.pending = []
    def predict(self, now): return None if self.val is None else max(0.0, self.val - (now - self.at))
    def feed(self, reading, now):
        """Returns 'ok', 'resync' or None (ignored)."""
        if reading is None or not 0 <= reading <= max(self.duration, MAX_CLOCK): return None
        p = self.predict(now)
        if p is None:
            self.pending.append((reading, now)); self.pending = self.pending[-3:]
            if len(self.pending) >= 2 and abs(self.pending[-1][0] - (self.pending[-2][0] - (now - self.pending[-2][1]))) <= 2.5:
                self.val, self.at, self.pending = reading, now, []; return 'ok'
            return None
        if abs(reading - p) <= 2.5:
            self.val, self.at, self.pending = reading, now, []; return 'ok'
        self.pending.append((reading, now)); self.pending = self.pending[-3:]
        if len(self.pending) == 3:                                      # three new readings agree with each other
            (r1, t1), (r2, t2), (r3, t3) = self.pending
            if abs(r2 - (r1 - (t2 - t1))) <= 2 and abs(r3 - (r2 - (t3 - t2))) <= 2 and r3 < p:
                self.val, self.at, self.pending = r3, t3, []; return 'resync'   # only forward: the clock never runs back
        return None

# ---------------------------------------------------------------- fight
CHARACTERS = os.path.join(HERE, 'characters.json')   # {uid: name}, so the name survives a restart

def _characters_file():
    """characters.json: {uid: {'name', 'verified'}}; older versions wrote {uid: name}."""
    try:
        with open(CHARACTERS, encoding='utf-8') as fh: raw = json.load(fh)
    except (OSError, ValueError): return {}
    return {uid: v if isinstance(v, dict) else {'name': v, 'verified': False} for uid, v in raw.items() if v}

def known_characters():
    return {uid: v.get('name') for uid, v in _characters_file().items() if v.get('name')}

def character_verified(uid):
    """True once the name was read from the game menu, right above this UID."""
    return bool(uid and _characters_file().get(uid, {}).get('verified'))

def same_name(a, b):
    """Two OCR reads of one in-game name (unique per player): equal ignoring case, or off by a misread letter or two."""
    a, b = (a or '').strip().casefold(), (b or '').strip().casefold()
    return bool(a and b) and (a == b or (min(len(a), len(b)) >= 4 and difflib.SequenceMatcher(None, a, b).ratio() >= 0.85))

TEAMMATES = os.path.join(HERE, 'teammates.json')
STEADY_SEEN = 3        # a teammate's name read this many times (separate Party screens) is that player's name
STEADY_ALIKE = 0.85    # a read at least this alike to it is a misread of it ('Kyhra' isn't Kyhru: 0.8)

class Teammates:
    """Teammates' names as seen on the Party screen, run after run, so one player goes to the leaderboard under one
    name. The game never shows a teammate's UID (the site ties a name to a UID only once that player runs aLogs and
    claims it), so a misread ('AkArashi', 'AFKAFashi' for AFKArashi) made a new player on the boards each time. A
    name read clearly on STEADY_SEEN Party screens is kept; a later read that is a near miss of it, and hasn't been
    seen twice in its own right, is sent as that name. Short names (under 4 characters) are left exactly as read."""
    def __init__(self, path=TEAMMATES):
        self.path = path
        try:
            with open(path, encoding='utf-8') as f: self.seen = {k: int(v) for k, v in json.load(f).items()}
        except (OSError, ValueError, AttributeError): self.seen = {}

    @staticmethod
    def fold(name):
        """The name as lookalike letters fold together (accents dropped, 0/O/o, 1/l/I/|), for comparing reads."""
        import unicodedata
        plain = ''.join(c for c in unicodedata.normalize('NFKD', name or '') if not unicodedata.combining(c))
        return plain.casefold().translate(str.maketrans({'0': 'o', '1': 'l', 'i': 'l', '|': 'l'}))

    def steady(self, name):
        if not name or len(name) < 4 or self.seen.get(name, 0) >= 2: return name
        a = self.fold(name)
        best = max(((difflib.SequenceMatcher(None, a, self.fold(k)).ratio(), n, k) for k, n in self.seen.items()
                    if n >= STEADY_SEEN and k != name and len(k) >= 4), default=None)
        if best and best[0] >= STEADY_ALIKE:
            log.info(f'teammate: {name!r} read as {best[2]!r} (seen {best[1]} times)')
            return best[2]
        return name

    def saw(self, name):
        if not name or len(name) < 2: return
        self.seen[name] = self.seen.get(name, 0) + 1
        try: S.write_json(self.path, self.seen, indent=1, ensure_ascii=False)
        except OSError: pass

TEAM = Teammates()

def vote(votes, key, value):
    """One read's value for `key` into `votes`; returns the value most reads so far agree on (the first seen on a tie;
    a read that found nothing doesn't vote)."""
    if value not in (None, ''): votes.setdefault(key, collections.Counter())[value] += 1
    c = votes.get(key)
    return max(c, key=lambda v: (c[v], -list(c).index(v))) if c else value

def party_pill(pill, i):
    """A Party screen pill's read ('2P @ Flora O': the badge, a ready mark, the name, the voice icon) as (its slot, the
    name), the pill being the i-th; None when no name is left."""
    # the text model reads the badge onto the name ('1PMinMax'): a space after it, so it's dropped below. Its P can read
    # small ('4pR'), and a speck can come before it ('.4pR'): kept as a name, the badge glued a one-character name into
    # a 3-4 letter one on 2 of 10 saved captures of 1 Nov (5 of 1508 pills read in the logs; a sixth kept '.2P' on one)
    pill = re.sub(r'^[\s.,]+(?=[1-4]\s*[Pp]|[Il]\s*P)', '', pill)
    pill = re.sub(r'^\s*([1-4]\s*[Pp]|[Il]\s*P)(?=[^\s/|])', r'\1 ', pill)
    # the name: its words joined (OCR splits 'MiraVosk' as 'Mira Vosk'); badge, marks and icons left out - they read
    # as one letter, or two odd ones ('iA')
    words = [w for w in re.split(r'[\s/|]+', pill) if not re.fullmatch(r'[1-4][Pp]|[Il]P', w)
             and (len(w) >= 3 or (len(w) == 2 and w[0].isupper() and not w[1].isupper()) or K.has_hangul(w))]
    name = ''.join(words)
    if not name: return None
    # the slot is the pill's own badge ('3P'), not where the pill sits: not being 1P may move you to the middle,
    # and the result screen and Battle Data list players by badge, 1P to 4P (the pill's place if it misread)
    badge = re.match(r'\s*([1-4])\s*[Pp]|\s*([Il])\s*P', pill)
    return (int((badge.group(1) or badge.group(2)).translate(str.maketrans('Il', '11'))) - 1 if badge else i), name

def sure_read(ocr, img):
    """Is this name box's read in the name cache for good? A read of one character or none only is when the Korean
    model vouched for it (Ocr.read_native: KoreanNames.lone, font-checked, or a Hangul name the font settled)."""
    hit = getattr(ocr, 'name_cache', {}).get(R.name_signature(img))
    return bool(hit) and hit[1] is None

def party_slots(players, party, sure=(), you=None):
    """Each result-screen (or Battle Data) column takes the Party screen's player of the same slot: both list players
    by badge, 1P to 4P, left to right (a run as 2P with three real players lined up on all three screens, 30 Oct). The
    name comes from the Party screen, read clearly there, with its AI flag and you; the column's own read only checks it.
    A misread like 'Dyn*ht' no longer has to be guessed at. A column that clearly reads as another member's name (the
    order wasn't what was expected) takes that member instead, and it's logged. No party: names stay as read.
    Each member goes to one column at most: first to a column that clearly reads their name, then to their slot's
    column; a column whose slot's member went to another keeps its own read, with no AI flag or you from the party.
    Given to both (by name to one, by slot to the other), one player was listed twice in 7 sent runs of 1 Nov (their
    Battle Data cards too), 2 of them with two rows marked as you. Names are compared folded (TEAM.fold: 0 as O, 1 as
    l): in 5 saved runs a column read a member's name that only folding matches, and stayed unmatched.
    Two columns keep their own read: one that reads as `you` (your character name, when the game menu showed it) is
    you, and no other column takes the Party screen's you; one of one Latin letter or digit the Korean model vouched
    for (`sure`: those slots, sure_read) is that player. The Party screen's read of either slot can only be worse: its
    filter never keeps a one-character name, and on its other layout (pills ~0.018 H higher, before everyone is ready)
    its boxes read the title above or half a pill. 6 of 59 sent runs had no row marked as you (the result screen read
    your exact name in all 6), and 4 of 10 saved runs of 1 Nov sent a one-character teammate's misread."""
    if not party:
        for p in players:
            if p.get('name') and not p.get('self'): p['name'] = TEAM.steady(p['name'])
        return
    by_slot = {m['slot']: m for m in party}
    sim = lambda a, b: difflib.SequenceMatcher(None, TEAM.fold(a), TEAM.fold(b)).ratio()
    taken, rest = set(), []                      # members (by slot) given to a column; the columns left to settle
    # the Party screen's you: its mark, or your name. Its vote keeps a slot's commonest name but the last read's mark,
    # so a shuffled read can leave your name unmarked in one slot and the mark on another (1 saved run)
    mine = {x['slot'] for x in party if x.get('self') or (you and same_name(x['name'], you))}
    def give(p, m):                              # a member taken as you takes every Party you: never two rows
        p.update(name=m['name'], ai=bool(m.get('ai')), self=bool(m.get('self')))
        taken.update(mine if m['slot'] in mine else {m['slot']})
    me = max((p for p in players if you and same_name(p.get('name'), you)), key=lambda p: sim(p['name'], you), default=None)
    for p in players:
        own, m = p.get('name') or '', by_slot.get(p['slot'])
        if p is me:
            if m and m['name'] != you: log.info(f"result: column {p['slot'] + 1} reads as you, not the Party screen's {m['name']!r}")
            p.update(name=you, ai=False, self=True); taken.update(mine)
        elif p['slot'] in sure and len(own) == 1 and own.isascii() and own.isalnum():
            if m and m['name'] != own: log.info(f"result: column {p['slot'] + 1} reads {own!r} (sure), not the Party screen's {m['name']!r}")
            p.setdefault('ai', False)
        else: rest.append(p)
    def named(p):                                # the free member this column clearly reads, when not its slot's
        m = by_slot.get(p['slot'])
        other = max((x for x in party if x is not m and x['slot'] not in taken), key=lambda x: sim(x['name'], p['name']), default=None)
        s = sim(other['name'], p['name']) if other else 0
        return (s, other) if s >= 0.8 and (m is None or s >= sim(m['name'], p['name']) + 0.3) else None
    while True:                                  # the clearest read first, when two columns read one member
        hits = [(h, p) for p in rest for h in [named(p)] if h]
        if not hits: break
        (s, other), p = max(hits, key=lambda h: h[0][0])
        m = by_slot.get(p['slot'])
        log.info(f"result: column {p['slot'] + 1} reads {p['name']!r}, the Party screen's {other['name']!r} (slot {other['slot'] + 1}), not slot {p['slot'] + 1}'s {m and m['name']!r}")
        give(p, other); rest.remove(p)
    for p in rest:
        m = by_slot.get(p['slot'])
        if m is None: continue
        if m['slot'] in taken:
            log.info(f"result: column {p['slot'] + 1} keeps its read {p['name']!r}: slot {p['slot'] + 1}'s {m['name']!r} is another column's")
            if p['name']: p['name'] = TEAM.steady(p['name'])
            continue
        if p['name'] and sim(m['name'], p['name']) < 0.3: log.info(f"result: column {p['slot'] + 1} reads {p['name']!r}, taken as slot {p['slot'] + 1}'s {m['name']!r}")
        give(p, m)

def mark_you(players, party, known):
    """After party_slots, on the result screen: your row is the one it marked, else the first that reads as your known
    name or as the Party screen's you; never a second one. Each row's AI flag, when party_slots gave none, is the Party's
    for its name."""
    you = next((m['name'] for m in party or [] if m.get('self')), None)
    mine = any(p.get('self') for p in players)
    for p in players:
        if not mine and ((known and same_name(p['name'], known)) or (you and p['name'] == you)): p['self'] = mine = True
        p['self'] = bool(p.get('self'))
        p.setdefault('ai', any(m['ai'] and m['name'] == p['name'] for m in party or []))

def own_name(known, names, party):
    """Which name on the result screen is the player running the meter. You aren't always the 1P slot (in someone
    else's party you're 2P-4P), so only a sure answer counts: your saved name is among the slots, or the prep screen
    showed every other member as AI, which leaves you as the one real player. None when it can't be told."""
    names = [n for n in names if n and 2 <= len(n) <= 40]
    if known and any(same_name(known, n) for n in names): return known
    humans = [m['name'] for m in party or [] if not m['ai']]
    if party and len(humans) == 1:
        return next((n for n in names if same_name(humans[0], n)), humans[0] if 2 <= len(humans[0]) <= 40 else None)
    return None

def remember_character(uid, name, verified=False):
    """Save the name for this UID. A name read from the game menu (verified) is never replaced by a guess from a
    result screen; a newer menu reading replaces it (renamed character)."""
    if not uid or not name: return
    known = _characters_file()
    old = known.get(uid, {})
    if old.get('verified') and not verified: return
    if old.get('name') == name and old.get('verified') == verified: return
    known[uid] = {'name': name, 'verified': verified}
    try:
        S.write_json(CHARACTERS, known, ensure_ascii=False)
    except OSError: pass

BAR_SPOT = os.path.join(HERE, 'bar_spot.json')

def screen_sig(im):
    """A 48x27 grey thumbnail of the frame (well under a millisecond), to notice the screen changing."""
    return im.resize((48, 27), Image.NEAREST).convert('L').tobytes()

def screen_changed(a, b, limit=24):
    """Did the screen change (another screen opened or this one closed), not just animate? Mean difference per pixel."""
    return sum(abs(x - y) for x, y in zip(a, b)) / len(a) > limit

IMAGE_TYPES = ('.png', '.jpg', '.jpeg', '.webp', '.bmp')
TEXT_DIAGNOSTICS = ('battle_data', 'nobar', 'bar_lost')   # their .txt (what was read off the screen, other players' names too) go
                                              # with the pictures: the records keep what's needed
TEST_FRAME_DAYS = 7                 # Save test pictures: each day's folder is kept this long...
TEST_FRAME_BYTES = 3 << 30          # ...and all of them together at most this much (oldest days go first)
BATTLE_DATA_WAIT = 45               # seconds a best run waits for Battle Data before it goes with the result screen alone
RESULT_PAUSE = 5                    # seconds the automatic Battle Data session stays on the result screen before clicking,
BATTLE_DATA_PAUSE = 5               # and holds on Battle Data once it's open, so each is read many times (inputs held)
BATTLE_DATA_SETTLE = 6              # ...then a run is decided this long after Battle Data is in, once those reads voted
SCREEN_READS = 10                   # reads of each screen per appearance; the value most reads agree on wins
SIM_GATE_GONE = 3           # s with the exit gate's button gone before a press no longer counts as ending the run
SIM_PRESS_EARLY = 1.0       # s before the exit gate's button was first seen that a press still counts (it shows first)
SIM_VICTORY_WAIT = 8        # s after your press at the exit gate for the Victory screen (it comes in ~1 s) before control comes back
HOLD_MIN = RESULT_PAUSE + BATTLE_DATA_PAUSE   # "Block for at most" never shorter than both pauses (10 s)
FIGHT_LOG_DAYS = 30                 # each fight's per-frame CSV is kept this long (its result in logs/fights stays)
LEFTOVER_HOURS = 6                  # "only until its data is saved": anything a crash or diagnostics left behind
KEEP_FOLDERS = {'submit_queue', 'fights', 'characters'}   # working folders the cleanup never removes, even when empty

def clean_old_screenshots(keep_hours):
    """Delete the meter's screenshots older than keep_hours (DPS Meter > Captures & logs > Keep screenshots): result,
    prep and Battle Data screens, end-of-fight and no-bar captures, and each fight's boss-bar snapshots. The records
    themselves (CSV, JSON) are tiny and stay, and so does the game-menu proof (logs/characters, one per character).
    Folders left empty go too. Returns (files, bytes) removed."""
    if keep_hours is None or keep_hours < 0: return 0, 0        # keep everything (opt-in)
    cutoff = time.time() - (keep_hours or LEFTOVER_HOURS) * 3600   # "until saved": leftovers go after a few hours
    root = os.path.join(HERE, 'logs'); files = size = 0
    # kept whatever their age: proof a queued record still has to send (offline, or the server busy), and the test
    # pictures the player saves to send to the developers (Captures & logs › Save test pictures)
    needed = set()
    for q in (os.listdir(os.path.join(root, 'submit_queue')) if os.path.isdir(os.path.join(root, 'submit_queue')) else []):
        try:
            with open(os.path.join(root, 'submit_queue', q), encoding='utf-8') as f: item = json.load(f)
            for key in ('evidence', 'battle', 'proof'):
                if isinstance(item, dict) and item.get(key):
                    needed.add(os.path.normcase(os.path.abspath(os.path.join(HERE, item[key]))))
        except (OSError, ValueError): pass
    keep_dirs = [os.path.normcase(os.path.join(root, k)) for k in ('characters', 'test_frames')]
    for d, dirs, names in os.walk(root, topdown=False):
        if any(os.path.normcase(d).startswith(k) for k in keep_dirs): continue
        for n in names:
            if not (n.lower().endswith(IMAGE_TYPES)
                    or (n.lower().endswith('.txt') and os.path.basename(d) in TEXT_DIAGNOSTICS)): continue
            p = os.path.join(d, n)
            if os.path.normcase(os.path.abspath(p)) in needed: continue
            try:
                st = os.stat(p)
                if st.st_mtime < cutoff: os.remove(p); files += 1; size += st.st_size
            except OSError: pass
        if d != root and os.path.basename(d) not in KEEP_FOLDERS:
            try: os.rmdir(d)                           # only succeeds when the folder is now empty
            except OSError: pass
    return files, size

def clean_old_fight_logs(days=FIGHT_LOG_DAYS):
    """Each fight's per-frame CSV (logs/<fight>.csv, diagnostics) after `days`; the fight's result (logs/fights) stays."""
    root = os.path.join(HERE, 'logs'); cutoff = time.time() - days * 86400; files = size = 0
    try: names = os.listdir(root)
    except OSError: return 0, 0
    for n in names:
        p = os.path.join(root, n)
        if not (n.endswith('.csv') and n[:8].isdigit()): continue          # 20261027_011232_AlphaIrisal.csv
        try:
            st = os.stat(p)
            if st.st_mtime < cutoff: os.remove(p); files += 1; size += st.st_size
        except OSError: pass
    return files, size

def clean_test_frames(days=TEST_FRAME_DAYS, limit=TEST_FRAME_BYTES):
    """Save test pictures (logs/test_frames/<day>/): whole 4K frames add up to gigabytes a day, so a day's folder
    goes after `days`, and the oldest days first while all of them are over `limit`. Returns (files, bytes) removed."""
    import shutil
    root = os.path.join(HERE, 'logs', 'test_frames')
    try: day_names = sorted(n for n in os.listdir(root) if n.isdigit() and len(n) == 8)
    except OSError: return 0, 0
    cutoff = time.strftime('%Y%m%d', time.localtime(time.time() - days * 86400))
    sizes = {}
    for n in day_names:
        found = [os.path.join(at, f) for at, _, fs in os.walk(os.path.join(root, n)) for f in fs]
        total = 0
        for f in found:
            try: total += os.path.getsize(f)
            except OSError: pass
        sizes[n] = (len(found), total)
    files = size = 0; left = sum(b for _, b in sizes.values())
    for n in day_names:
        if n >= cutoff and left <= limit: continue
        if n == time.strftime('%Y%m%d') and n >= cutoff: break     # never today's, while it's being saved
        shutil.rmtree(os.path.join(root, n), ignore_errors=True)
        files += sizes[n][0]; size += sizes[n][1]; left -= sizes[n][1]
    return files, size

def screenshot_cleanup(meter):
    """At start and every hour: clear out old screenshots (the setting can change while the meter runs) and old
    per-frame fight logs."""
    while not meter.stop:
        try:
            hours = int(C.load()['capture'].get('keep_hours', 0))
            files, size = clean_old_screenshots(hours)
            if files: log.info(f'removed {files} screenshots older than {hours or LEFTOVER_HOURS} hours ({size / 1048576:,.0f} MB)')
            files, size = clean_old_fight_logs()
            if files: log.info(f'removed {files} per-frame fight logs older than {FIGHT_LOG_DAYS} days ({size / 1048576:,.1f} MB)')
            files, size = clean_test_frames()
            if files: log.info(f'removed {files} test pictures (older than {TEST_FRAME_DAYS} days or over {TEST_FRAME_BYTES >> 30} GB, {size / 1048576:,.0f} MB)')
        except Exception as e:
            log.warning(f'screenshot cleanup: {e!r}')
        for _ in range(3600):
            if meter.stop: return
            time.sleep(1)

PROOF_BYTES = 1800 * 1000         # the site takes a character proof of at most 1.9 MB (its database row's limit is 2 MB)

def save_proof(pic, base):
    """A character proof as `base`.png, or as a JPEG (shrunk as needed) when the PNG is over the site's limit: the
    Profile screen's column at 4K came to 0.5-1.5 MB, over the site's old 512 KB, and links were turned down ("Request body is too
    large"). Returns the path."""
    import io
    for ext in ('.png', '.jpg'):
        try: os.remove(base + ext)
        except OSError: pass
    buf = io.BytesIO(); pic.save(buf, 'PNG', optimize=True)
    if buf.tell() <= PROOF_BYTES:
        with open(base + '.png', 'wb') as f: f.write(buf.getvalue())
        return base + '.png'
    quality = 90
    while True:
        buf = io.BytesIO(); pic.save(buf, 'JPEG', quality=quality)
        if buf.tell() <= PROOF_BYTES or pic.height < 400: break
        if quality > 75: quality -= 5
        else: pic = pic.resize((pic.width * 4 // 5, pic.height * 4 // 5), Image.LANCZOS)
    with open(base + '.jpg', 'wb') as f: f.write(buf.getvalue())
    return base + '.jpg'

HP_BAR = (0.400, 0.915, 0.604, 0.9519)    # your HP bar at the bottom middle with its number on it, in the 16:9 UI area
HP_TEXT = re.compile(r'(\d[\d ]*)\s*/\s*(\d[\d ]*)')

def hp_number(ocr, strip, u):
    """Your HP as the game writes it on the bar ("12643/12694") in a picture of HP_BAR: (current, max), or None."""
    w, h = strip.size
    text = ocr.read({'hp': R.prep_white(strip, (w * 0.30, h * 0.36, w * 0.70, h * 0.80), u / 2000)}).get('hp', '')
    m = HP_TEXT.search(text.replace('O', '0').replace('o', '0'))
    return (int(m.group(1).replace(' ', '')), int(m.group(2).replace(' ', ''))) if m else None

def save_picture(im, folder, name, width=1920):
    """A screen kept as a picture: a JPEG at most `width` px wide (about 0.3 MB at 1920; a 4K PNG is about 5 MB).
    Leaderboard proofs keep the screen's own size, so the cropped proof the site shows stays sharp."""
    d = os.path.join(HERE, 'logs', folder); os.makedirs(d, exist_ok=True)
    if im.width > width: im = im.resize((width, round(im.height * width / im.width)), Image.LANCZOS)
    path = os.path.join(d, name + '.jpg'); im.convert('RGB').save(path, quality=85); return path

SAMPLES_KEPT = 5           # pictures of each kind in logs/samples (the newest); they age out like every screenshot
                           # (clean_old_screenshots: Keep screenshots, e.g. 7 days, or a few hours when it's 0)
SAMPLE_GAP = 600           # seconds between two samples of one kind (one a run, not one a read)
_sample_at = {}

def keep_sample(im, kind):
    """A few full-size pictures of the screens the readers depend on (the Party screen, an Interlink fight's HUD),
    kept on this PC so a changed screen can be checked against real pictures later: the newest SAMPLES_KEPT of each
    kind, as PNG at the game's own size. Never sent anywhere."""
    now = time.time()
    if now - _sample_at.get(kind, 0) < SAMPLE_GAP: return
    _sample_at[kind] = now
    try:
        d = os.path.join(HERE, 'logs', 'samples'); os.makedirs(d, exist_ok=True)
        im.convert('RGB').save(os.path.join(d, f"{kind}_{time.strftime('%Y%m%d_%H%M%S')}.png"))
        for old in sorted(n for n in os.listdir(d) if n.startswith(kind + '_'))[:-SAMPLES_KEPT]: os.remove(os.path.join(d, old))
    except OSError as e: log.info(f'sample picture not kept: {e!r}')

def last_rush_level():
    """The level of the last Interlink boss fought (logs/fights/*.json), for the instant start."""
    import glob
    for path in sorted(glob.glob(os.path.join(HERE, 'logs', 'fights', '*.json')), reverse=True)[:30]:
        try:
            with open(path, encoding='utf-8') as f: s = json.load(f)
            if s.get('bossrush') and s.get('level'): return int(s['level'])
        except (OSError, ValueError, TypeError): continue
    return None

def refresh_cycles(meter):
    """The boss rush week schedule from the game's own data (gamedata.py), re-read when a game patch changes it. The
    meter uses it for this week's bosses; a developer account's app also sends it to the site so new weeks appear
    there without anyone updating the site. Runs in the background at start and every few hours."""
    import gamedata
    path = os.path.join(HERE, 'boss_rush_cycles.json')
    while not meter.stop:
        # Game data that doesn't have this week is an old copy (a second, older install): it gave old weeks' bosses and
        # HP and no new bosses (Glameep matched an open-world Omega Glameep; 30 Oct report). Then aLogs' own schedule
        # and boss list are used, and nothing is sent to the site.
        stale = False
        try:
            cycles, changed = gamedata.cached_cycles(os.path.join(HERE, 'game_cycles.json'), meter.args.player_log)
            with open(os.path.join(R.HERE, 'boss_rush_cycles.json'), encoding='utf-8') as f: shipped_cycles = json.load(f)
            stale = bool(cycles) and not gamedata.covers_now(cycles) and gamedata.covers_now(shipped_cycles)
            if stale:
                log.warning(f"the game data at {gamedata.find_game_data(meter.args.player_log)} is older than this week "
                            f"(latest week {cycles[-1]['cycle']}): using aLogs' own boss list and week schedule instead")
                R.GAME_BOSSES[0] = None; meter.catalog = R.load_catalog(); R.CYCLES_FILE[0] = os.path.join(R.HERE, 'boss_rush_cycles.json')
                R.GAME_GRADES[0] = None; meter.grades = R.load_grades()
        except Exception as e:
            log.warning(f'could not check the game data: {e}')
        if stale:
            for _ in range(6 * 3600):
                if meter.stop: return
                time.sleep(1)
            continue
        try:   # the boss list too: a patch that adds a boss needs no new aLogs build
            shipped = R.load_catalog() if R.GAME_BOSSES[0] is None else meter.catalog
            keep = [(r['table'], r['puppetId']) for r in shipped if r.get('monsterType') != '2']
            bosses, changed = gamedata.cached_bosses(os.path.join(HERE, 'game_bosses.json'), keep, meter.args.player_log)
            if bosses:
                had = {(r['table'], r['puppetId']) for r in meter.catalog}
                R.GAME_BOSSES[0] = bosses; meter.catalog = R.load_catalog()
                new = sorted({r['bossTitle_en'] or r['name_en'] for r in meter.catalog if (r['table'], r['puppetId']) not in had})
                if changed or new: log.info(f'boss list read from the game data: {len(bosses)} rows' + (f', new: {new[:10]}' if new else ''))
        except Exception as e:
            log.warning(f'could not read the boss list from the game data: {e}')
        try:   # Interlink's HP bars too (bossrush_grade_data): the Whisperwake update took them from 16 bars to 21
            grades, changed = gamedata.cached_grades(os.path.join(HERE, 'game_grades.json'), meter.args.player_log)
            if grades:
                R.GAME_GRADES[0] = grades; meter.grades = R.load_grades()
                if changed: log.info(f'Interlink HP bars read from the game data: {len(grades) - 1} bars')
        except Exception as e:
            log.warning(f'could not read the Interlink HP bars from the game data: {e}')
        try:
            cycles, changed = gamedata.cached_cycles(os.path.join(HERE, 'game_cycles.json'), meter.args.player_log)
            if cycles:
                # Written whenever it isn't what the game data says now, not only when the cache changed: the stale
                # check above already took that `changed`, and a file from an aLogs before 91d115c (no bossIds2) was
                # then never rewritten, so a two-boss fight's second boss had no slot (and an old week's id and HP)
                try:
                    with open(path, encoding='utf-8') as f: on_disk = json.load(f)
                except (OSError, ValueError): on_disk = None
                if on_disk != json.loads(json.dumps(cycles)):
                    S.write_json(path, cycles)
                    log.info(f'boss rush weeks read from the game data: {len(cycles)} (latest {cycles[-1]["cycle"]})')
                R.CYCLES_FILE[0] = path
                upload_cycles(meter, cycles, changed)
        except Exception as e:
            log.warning(f'could not read the boss rush weeks from the game data: {e}')
        for _ in range(6 * 3600):
            if meter.stop: return
            time.sleep(1)

def upload_cycles(meter, cycles, changed):
    """Developer accounts: send the schedule to the site once per game data version (the site refuses others)."""
    import urllib.request, urllib.error
    config = S.load_config()
    marker = os.path.join(HERE, 'game_cycles.uploaded')
    stamp = ''
    try:
        with open(os.path.join(HERE, 'game_cycles.json'), encoding='utf-8') as f: stamp = json.load(f).get('stamp', '')
        with open(marker, encoding='utf-8') as f: done = f.read().strip()
    except OSError: done = ''
    if not config or (done == stamp and not changed): return
    names = {r['puppetId']: r['bossTitle_en'] for r in meter.catalog if r['table'] == 'puppet_data' and r['bossTitle_en']}
    body = {'cycles': [{'id': c['cycle'], 'season': c.get('season'), 'start': c['start'], 'end': c['end'],
                        'bosses': [{'id': str(b), 'name': names.get(str(b), str(b))} for b in c['bossIds']],
                        # left / middle / right as in 'bosses'; a two-boss fight's second boss, and the week's split
                        'bosses2': [{'id': str(b), 'name': names.get(str(b), str(b))} if b else None for b in c.get('bossIds2') or [None] * 3],
                        'split': c.get('split')}
                       for c in cycles if c.get('start') and c.get('end')]}
    req = urllib.request.Request(f"{config['api_url']}/leaderboard/interlink/cycles", data=json.dumps(body).encode(),
                                 method='POST', headers={'authorization': f"Bearer {config['token']}",
                                                         'content-type': 'application/json', 'user-agent': S.CLIENT_VERSION})
    try:
        with urllib.request.urlopen(req, timeout=20) as r: log.info(f'boss rush weeks sent to the site: HTTP {r.status}')
        with open(marker, 'w', encoding='utf-8') as f: f.write(stamp)
    except urllib.error.HTTPError as e:
        if e.code == 403:                                        # not a developer account: nothing to do, ever
            log.info('boss rush weeks: only developer accounts update the site')
            with open(marker, 'w', encoding='utf-8') as f: f.write(stamp)
        else: log.warning(f'boss rush weeks: the site answered HTTP {e.code}')
    except (urllib.error.URLError, OSError) as e:
        log.warning(f'boss rush weeks: could not reach the site ({e})')

class BreakTracker:
    """The break gauge over a fight. It drains as break damage lands; at zero the boss is broken for a while, then the
    gauge refills and drains again. Break per second = how fast the team drains it (% of the gauge per second), over
    the whole fight and over the last few seconds; it also counts breaks and how long each took from a full gauge."""
    def __init__(self, size=None):
        self.size = size             # break points in a full gauge (from the game data), or None
        self.phase = 'drain'; self.start = None; self.prev = None; self.peak = 0.0
        self.recent = collections.deque(); self.breaks = 0; self.drained = 0.0; self.drain_time = 0.0
        self.last_after = None; self.left = None; self.kind = None

    def feed(self, g, kind, now):
        if g is None: return
        prev, self.prev, self.left, self.kind = self.prev, g, g, kind
        if self.phase == 'drain':
            if self.start is None: self.start = (now, g); self.recent.clear()
            if prev is not None and g > prev + 0.12:                  # refilled without a break we saw
                self.phase = 'refill'; self.peak = g; self.start = None
            elif g <= 0.02 and prev is not None and prev <= 0.5:
                t0, g0 = self.start
                if now - t0 > 1: self.drained += g0; self.drain_time += now - t0
                self.breaks += 1; self.last_after = now - t0 if g0 > 0.9 else None
                log.info(f'break #{self.breaks}' + (f' after {self.last_after:.0f} s' if self.last_after else ''))
                self.phase = 'broken'; self.start = None
            else:
                self.recent.append((now, g))
                while self.recent and now - self.recent[0][0] > 5: self.recent.popleft()
        elif self.phase == 'broken':
            if g >= 0.05: self.phase = 'refill'; self.peak = g
        elif self.phase == 'refill':
            self.peak = max(self.peak, g)
            if g >= 0.97 or g < self.peak - 0.03: self.phase = 'drain'; self.start = (now, g); self.recent.clear()

    def rate(self, now):
        """(% per second over the fight, % per second over the last ~5 s) of draining, or None while unknown."""
        drained, t = self.drained, self.drain_time
        if self.phase == 'drain' and self.start and self.left is not None:
            drained += max(0.0, self.start[1] - self.left); t += now - self.start[0]
        overall = drained / t * 100 if t > 3 else None
        (ta, ga), (tb, gb) = (self.recent[0], self.recent[-1]) if len(self.recent) > 1 else ((0, 0), (0, 0))
        recent = max(0.0, ga - gb) / (tb - ta) * 100 if tb - ta >= 2 else None
        return overall, recent

    def info(self, now):
        overall, recent = self.rate(now)
        return {'left': self.left, 'phase': self.phase, 'kind': self.kind, 'rate': overall, 'rate_now': recent,
                'breaks': self.breaks, 'last_after': self.last_after, 'size': self.size}

HANDOFF_XN = 3          # a two-boss fight's second boss counts as on the bar from its counter's top 4 bars (x21-x18):
                        # each is 0.23% of its HP (bossrush_grade_data rows 1-7, bossHpLossPercent_3), ~0.3 s apiece
FIGHT_CSV = ['wall_s', 'timer_s', 'timer_ocr', 'xn_ocr', 'segment', 'fill_pct', 'fill_rgb', 'edge_rgb',   # logs/<fight>.csv
             'team_damage', 'avg_dps', 'dps_5s', 'note', 'break_left_pct', 'break_phase']

class Fight:
    """One run on one boss bar. A two-boss middle fight (weeks 4003-4005: bossrush_guanka_data rows with a bossId_2)
    is one run in two phases: the stage's level graph starts its single 180/240 s clock as the first boss enters
    combat and nothing on its two-boss path restarts it (Scene.1070 scene_graph_data[78824533] nodes 5-11), the
    second boss spawns only on the first one's death (nodes 16-19), and the result screen is one run
    (BossRushBattleResultCtrl.initUI, line 60). So the Timer, the clock, the time limit and the CSV are the run's,
    while the bar's own state (segment, counter votes, refills, break gauge) is each boss's. max_hp and damage are the
    run's: the bosses' max HP so far and the damage to them (the first boss's max HP once it is down, plus the
    current boss's damage), so max_hp - damage is still the HP left on the bar; boss_hp is the current boss's."""
    def __init__(self, title, level, row, max_hp, bossrush, debug_dir=None, partner=None, order=None):
        self.title, self.level, self.row, self.max_hp, self.bossrush = title, level, row, max_hp, bossrush
        self.t0 = self.created = time.time(); self.damage = 0; self.samples = collections.deque()
        # partner: the catalog row of the boss that follows this one in a two-boss fight (R.stage_of order 1), else
        # None; order: the boss's place in its stage (R.stage_of: 1, 2, None on one-boss stages)
        self.partner, self.order = partner, order
        self.boss_hp = max_hp        # the max HP of the boss now on the bar
        self.done_hp = 0             # the max HP of the bosses already down in this run (a two-boss fight's first)
        # each boss of the run: t0/t1 wall time, start_s/end_s on the run's clock (elapsed)
        self.phases = [{'row': row, 'title': title, 'max_hp': max_hp, 'damage': 0, 'defeated': False,
                        't0': self.created, 't1': None, 'start_s': 0.0, 'end_s': None, 'breaks': 0}]
        self.joined = False          # a run first seen at its second boss: that boss's start on the run's clock is set
        # Out in the world a boss's bar shows as soon as you're near it, often before anyone attacks: the fight's
        # clock starts at the first hit (between the last reading before the damage and the one that shows it).
        # Interlink's clock runs from the start, like the game's own timer.
        self.hit = bossrush; self.first_damage = None; self.last_at = self.created
        self.low_pending = None      # (fill, time) of a big one-frame drop waiting for a later frame to confirm it
        self.away = 0.0              # seconds of this fight with the game in the background (you tabbed out)
        self.seg = None              # xN of the segment we are in (boss rush)
        self.seg_fill = 1.0          # lowest fill seen in this segment (damage never goes back)
        self.last_fill = None; self.xn_votes = []; self.vote_at = 0.0; self.timer = None; self.elapsed = 0.0; self.avg = self.recent = 0.0
        # the run's own time limit, read off the game's clock as the fight starts (3:00 by default, 4:00 with the
        # Overtime blessing, more if the game's default goes up); None when the meter didn't see the start
        self.limit = None
        self.started_at_top = False; self.refill_pending = 0; self.refill_at = 0.0; self.seg_info = None; self.defeated = False
        # the grade table column this boss's bars follow (R.grade_column: the two bosses of a two-boss fight have their own)
        self.column = R.grade_column(row.get('puppetId')) if bossrush else R.GRADE_BASE
        self.brk = BreakTracker(R.boss_break_points(row, level))
        name = time.strftime('%Y%m%d_%H%M%S') + '_' + ''.join(c for c in title if c.isalnum())[:24]
        os.makedirs(os.path.join(HERE, 'logs'), exist_ok=True)
        self.log = open(os.path.join(HERE, 'logs', name + '.csv'), 'w', newline='', encoding='utf-8')
        self.w = csv.writer(self.log)
        # a two-boss stage's run keeps one CSV for both bosses, each row marked with the boss on the bar (its spawn
        # order, 1 or 2: R.stage_of); a one-boss fight's CSV keeps the columns it always had
        self.boss_col = order is not None
        self.w.writerow(FIGHT_CSV + (['boss'] if self.boss_col else []))
        self.frames = os.path.join(HERE, 'logs', name) if debug_dir else None
        if self.frames: os.makedirs(self.frames, exist_ok=True)

    def update(self, fill, fill_rgb, edge_rgb, xn_ocr, timer_ocr, grades, duration, at=None):
        now = at or time.time(); note = ''          # at: when the game showed the frame (wincapture's frame_time)
        if self.timer is None: self.timer = Timer(duration)
        cum = grades.get(self.column) or grades[R.GRADE_BASE]   # grades: R.load_grades(), the meter's current table
        top = len(cum) - 1                                       # the counter at full HP: x21 (R.xn_top)
        if xn_ocr is not None and not 1 <= xn_ocr <= top: xn_ocr = None
        # a real reading starts with the live-HP red at the bar's left end; flashes, the ghost covering the whole
        # bar, or the yellow BREAK overlay give other colors - those frames carry no fill information
        r_, g_, b_ = fill_rgb
        if not (r_ >= 225 and 80 <= g_ <= 140 and 75 <= b_ <= 135): fill = None
        # --- segment tracking. Two signals: the counter (digit matcher every frame + OCR) and the bar refilling.
        #     A refill must hold for two frames; two agreeing counter reads can correct the segment either way.
        #     The counter is the authority. A refill only moves one bar past what the counter last confirmed: on
        #     30 Oct a misread start (x11 for the top bar) plus a fake refill (a low fill under an overlay for ~0.5 s, then
        #     the true fill back) put the meter 6 bars ahead, more than the counter may pull back up, and it stayed ahead
        #     to the end (x1 and 51.4M shown, the counter said x7: ~9M). Early bars really do pass every 2-4 s (x21 is
        #     0.44% of max HP, x1 is 50% on a one-boss stage), so the time between refills can't tell a fake one; the
        #     counter can.
        if self.bossrush:
            if xn_ocr and now - self.vote_at >= CONFIRM_GAP:
                self.xn_votes.append(xn_ocr); self.xn_votes = self.xn_votes[-4:]; self.vote_at = now
            agreed = xn_ocr if xn_ocr and self.xn_votes.count(xn_ocr) >= 2 else None
            if agreed and agreed == self.seg: self.xn_anchor = (agreed, now)
            if self.handoff_due(agreed, top):
                # a two-boss fight's first boss at its last bar, and the counter back up at the top: its partner holds
                # the bar now (the game resets xN to x21 for it: ClientBossRushDungeon.onSecondBossActivated, line 202).
                # Not a refill, and the bar need not vanish in between (BossTitleItemV2.refreshBossTitle, line 345)
                last = self.seg
                self.next_boss(self.partner, f'its bar came back at x{agreed} after x{last}')
                cum = grades.get(self.column) or grades[R.GRADE_BASE]
                self.seg = agreed; self.xn_anchor = (agreed, now); self.xn_votes = [agreed]; self.vote_at = now
                self.seg_fill = fill if fill is not None else 1.0
                note = f'segment x{agreed} (counter, boss {len(self.phases)})'
            elif self.seg is None:
                # Interlink starts at the top bar, x21. While the bar fades in, the start read as x11 (10 of 11 fights
                # on 30 Oct, two agreeing reads 15-30 s in; x21 then fell outside the old x16 cap, so probably x21 with
                # its 2 misread) or x6: anything but the top takes three reads and stays a guess that a later top read
                # can overrule (a meter that joins a fight late still gets the right segment from three reads)
                if xn_ocr == top or (agreed and self.xn_votes.count(agreed) >= 3):
                    self.seg = xn_ocr; note = f'segment x{xn_ocr} (counter)'; self.xn_anchor = (xn_ocr, now); self.seg_guess = xn_ocr != top
                    # from the run's start only: never a two-boss fight's second boss (its own top bar comes mid-run)
                    if len(self.phases) == 1 and self.order != 2:
                        self.started_at_top = xn_ocr == top and fill is not None and fill > 0.97
                    self.seg_fill = fill if fill is not None else 1.0
            # within a fight the counter only counts down: going up (x1->x4 reset the damage in 6 of 13 fights) takes
            # three agreeing reads instead of two, and at most 3 bars - except from a start guess to the top bar
            elif agreed and agreed != self.seg and (abs(agreed - self.seg) <= 3 or (agreed == top and getattr(self, 'seg_guess', False))) \
                    and (agreed < self.seg or self.xn_votes.count(agreed) >= 3):
                note = f'segment x{self.seg}->x{agreed} (counter)'; self.xn_anchor = (agreed, now); self.seg_guess = False
                self.seg = agreed; self.seg_fill = fill if fill is not None else 1.0; self.refill_pending = 0
            elif fill is not None and self.seg_fill < 0.25 and fill > 0.7 and fill - self.seg_fill > 0.45:
                if not self.refill_pending or now - self.refill_at >= CONFIRM_GAP:
                    self.refill_pending += 1; self.refill_at = now
                # the counter's word lapses after XN_STALE s without a read (a box we can't read must not freeze us)
                anchor, at = getattr(self, 'xn_anchor', None) or (None, 0.0)
                if anchor is not None and now - at > XN_STALE: anchor = None
                if self.refill_pending >= 2 and self.seg > 1 and (anchor is None or self.seg >= anchor):   # holds for two readings
                    note = f'segment x{self.seg}->x{self.seg - 1} (bar refilled)'; self.seg -= 1; self.seg_fill = fill
                    self.refill_pending = 0
                    self.xn_votes = [v for v in self.xn_votes if v <= self.seg]
                elif self.refill_pending >= 2 and self.seg > 1:
                    # already a bar past the counter: the low fill was the misread (the bar is back where it was), so
                    # the segment carries on from here; if it really was a refill the counter moves us down shortly
                    note = f'refill ignored, counter last x{anchor}'; self.seg_fill = fill; self.refill_pending = 0
            elif fill is not None:
                self.refill_pending = 0
        if fill is not None and not (self.bossrush and self.refill_pending):
            # Within a segment HP only drops, so one misread low frame (the bar sliding in, an effect over it) would
            # stick for the whole fight: a drop of more than 15% in one frame counts once a later frame confirms it.
            lp = self.low_pending; agrees = lp is not None and fill <= lp[0] + 0.02
            if fill < self.seg_fill - 0.15 and not (agrees and now - lp[1] >= CONFIRM_GAP):
                if not agrees: self.low_pending = (fill, now)
            else:
                self.seg_fill = min(self.seg_fill, fill); self.low_pending = None
        if fill is not None: self.last_fill = fill
        if self.timer.feed(timer_ocr, now) == 'resync': note = (note + ' timer resync').strip()
        # the clock's first trusted reading, a few seconds into a fight seen from its full bar, rounded up to 5 s. Only
        # in the run's first phase: a two-boss fight's clock is the run's (one 180/240 s clock, level graph 78824533
        # nodes 5-11), and what is left of it when the second boss shows up is no time limit
        if self.limit is None and self.bossrush and self.started_at_top and self.timer.val is not None and len(self.phases) == 1:
            if self.timer.at - self.created <= 4:
                self.limit = int(5 * math.ceil(self.timer.val / 5))
                if self.order is not None and self.limit not in (duration, duration + OVERTIME_S):
                    # A two-boss stage's level graph ends the run on one of two clocks only (Scene.1070
                    # scene_graph_data[78824533]: node 9 at 180 s, sys_config_data.BossRushCombatDuration, the meter's
                    # --duration; node 11 at 240 s with Overtime, bossrush_buff_data[208].additionalTime). A top bar
                    # with any other time left is not the run's start but its second boss at x21, the first one never
                    # seen (the fight started from the prep screen on the stage's first boss): no limit from it, and
                    # the run isn't tracked from its start
                    note = (note + f' {self.limit // 60}:{self.limit % 60:02d} left at a top bar mid-run: no time limit').strip()
                    self.limit = 0; self.started_at_top = False
                else: note = (note + f' time limit {self.limit // 60}:{self.limit % 60:02d}').strip()
            else: self.limit = 0                     # read too late to tell: the result screen's time decides
        # damage follows the current state, so a corrected segment also corrects the total. The bar shows the current
        # boss: its damage on its own max HP, after the bosses already down in this run. A second boss whose counter
        # isn't read yet adds nothing (its fill alone can't place it among its 21 bars)
        dmg, seg = R.damage_dealt(self.boss_hp, self.seg_fill, self.seg if self.bossrush else None, cum, self.bossrush)
        rose = False
        if dmg is not None and not (self.bossrush and self.seg is None and len(self.phases) > 1):
            rose = dmg > self.phases[-1]['damage']
            self.damage = self.done_hp + dmg; self.phases[-1]['damage'] = dmg
        if not self.hit:
            if self.first_damage is None:
                if fill is not None and self.low_pending is None: self.first_damage = self.damage   # from a real reading
            elif self.damage - self.first_damage > self.max_hp * 0.0012:             # a real hit (1 px of the bar is ~0.07%)
                # the hit came between the last reading and this one: the middle of the two (half the error)
                self.hit = True; self.t0 = (self.last_at + now) / 2
                note = (note + f' first hit, {self.t0 - self.created:.1f}s after the bar showed up').strip()
        self.last_at = now
        self.seg_info = seg                      # {'grade', 'size_hp', 'left_hp'} of the current star segment, or None
        if note: log.info(f'fight: {note} (fill {self.seg_fill * 100:.1f}%, damage {self.damage:,})')
        self.samples.append((now, self.damage))
        # keep the last sample from before the 5 s window: after a gap with no readings (a BREAK hides the bar for
        # seconds, then all its damage shows at once) the jump is spread over the time it really took, not one frame
        while len(self.samples) > 1 and now - self.samples[1][0] >= 5: self.samples.popleft()
        tp = self.timer.predict(now)
        clock = self.limit or duration
        self.elapsed = (clock - tp) if tp is not None and tp <= clock else (now - self.t0 if self.hit else 0.0)
        # each boss's time on the run's clock (run_of): its last hit seen, and where a run first seen at a two-boss
        # fight's second boss starts (the clock when its bar showed: the meter started late)
        if rose: self.phases[-1]['hit_s'] = round(self.elapsed, 1)
        if self.order == 2 and len(self.phases) == 1 and not self.joined and tp is not None and tp <= clock:
            self.joined = True; self.phases[0]['start_s'] = max(0.0, round(self.elapsed - (now - self.created), 1))
        avg = self.damage / self.elapsed if self.elapsed > 1 else 0
        (t_a, d_a), (t_b, d_b) = self.samples[0], self.samples[-1]
        self.recent = (d_b - d_a) / (t_b - t_a) if t_b - t_a > 0.5 else avg; self.avg = avg
        if not note and now - getattr(self, 'row_at', 0) < CONFIRM_GAP: return None   # ~10 rows a second is plenty
        self.row_at = now
        self.write([round(now - self.created, 2), None if tp is None else round(tp, 1), timer_ocr, xn_ocr, self.seg,
                    None if fill is None else round(fill * 100, 2), '%02x%02x%02x' % fill_rgb, '%02x%02x%02x' % edge_rgb,
                    self.damage, round(avg), round(self.recent), note,
                    None if self.brk.left is None else round(self.brk.left * 100, 1), self.brk.phase])
        return None

    def boss_of(self):
        """(the boss on the bar, of how many) on a two-boss stage: (1, 2) or (2, 2), its spawn order (R.stage_of), also
        for a run the meter first saw at its second boss; None for a one-boss fight. For the overlay and the app."""
        return (self.order, 2) if self.bossrush and self.order is not None else None

    def write(self, cells):
        """One row of the fight's CSV. A two-boss stage's run adds the boss on the bar (1 or 2) as its last column."""
        self.w.writerow(cells + [None] * (len(FIGHT_CSV) - len(cells)) + [self.order] if self.boss_col else cells)

    def mark_defeated(self, why):
        """The boss went down (killed or caught): the bar vanishes before the last hit can be read, so count the rest."""
        if self.defeated: return
        left = self.max_hp - self.damage
        self.defeated = True; self.damage = self.max_hp
        self.phases[-1].update(damage=self.boss_hp, defeated=True)
        self.avg = self.damage / self.elapsed if self.elapsed > 1 else 0
        try:
            self.write([round(time.time() - self.created, 2), None, None, None, self.seg, 0.0, '', '', self.damage,
                        round(self.avg), round(self.recent), f'defeated ({why}): final {left:,} HP counted'])
            self.log.flush()
        except ValueError: pass                                         # CSV already closed (confirmed after the fight)

    def partner_due(self):
        """A two-boss fight's first boss in its last three bars (x3-x1: its last 25% on bossHpLossPercent_2): its
        partner can take the bar any moment."""
        return self.partner is not None and self.seg is not None and self.seg <= 3

    def handoff_due(self, agreed, top):
        """The bar's own sign that a two-boss fight's first boss is down and its partner has the bar: the first boss in
        its last bar (x1, or x2 nearly empty: x1 is only 5% of its HP, bossrush_grade_data bossHpLossPercent_2 row 20)
        and two counter reads agreeing on one of the top bars. The counter only counts down within one boss, and the
        game puts the second boss back at x21 (BossTitleItemV2.switchBossRushTarget, line 484). Never from a start
        guess (seg_guess: a segment taken from three reads of a counter not at its top): while the first boss's bar
        fades in its x21 can read as x1, and the top read that follows is that boss's own start, which update's
        guess rule takes. Handed over then, the first boss counted as down at the run's start and its own bars were
        read on its partner's grade column (tests/two_boss.py scenario k: 51.4M for a run of 15.3M)."""
        return (self.partner is not None and agreed is not None and self.seg is not None and agreed >= top - HANDOFF_XN
                and not getattr(self, 'seg_guess', False)
                and (self.seg == 1 or (self.seg == 2 and self.seg_fill < 0.05)))

    def next_boss(self, row, why):
        """A two-boss fight's hand-off to the first boss's partner (`row`). The first boss is down: the graph spawns its
        partner only on its spawner's death (level graph 78824533 nodes 16-19), so its full max HP counts, as in
        mark_defeated. The run goes on: same Timer, clock, time limit and CSV; the bar's own state starts over for the
        partner, on its own grade column (R.grade_column) with its own break gauge."""
        now = time.time(); p = self.phases[-1]
        left = self.boss_hp - (self.damage - self.done_hp)
        # its time ends at its last hit seen (as it died), not with the gap before its partner shows up
        p.update(damage=self.boss_hp, defeated=True, t1=now, end_s=p.get('hit_s', round(self.elapsed, 1)), breaks=self.brk.breaks)
        self.done_hp += self.boss_hp
        hp = R.boss_max_hp(row, self.level)
        self.row, self.title, self.boss_hp, self.partner, self.order = row, row['bossTitle_en'] or self.title, hp, None, 2
        self.max_hp = self.done_hp + hp; self.damage = self.done_hp; self.defeated = False
        self.phases.append({'row': row, 'title': self.title, 'max_hp': hp, 'damage': 0, 'defeated': False,
                            't0': now, 't1': None, 'start_s': round(self.elapsed, 1), 'end_s': None, 'breaks': 0})
        self.seg = None; self.seg_fill = 1.0; self.seg_guess = False; self.seg_info = None; self.last_fill = None
        self.xn_votes = []; self.xn_anchor = None; self.refill_pending = 0; self.refill_at = 0.0; self.low_pending = None
        self.column = R.grade_column(row.get('puppetId'))
        self.brk = BreakTracker(R.boss_break_points(row, self.level))
        msg = (f'boss {len(self.phases)} of 2: {self.title}, max HP {hp:,} ({why}); the first boss is down, '
               f'its last {left:,} HP counted')
        log.info(f'fight: {msg} (run damage {self.damage:,}, {self.elapsed:.0f}s)')
        try:
            self.write([round(now - self.created, 2), None, None, None, None, None, '', '', self.damage,
                        round(self.avg), round(self.recent), msg])
        except ValueError: pass

    def close(self):
        p = self.phases[-1]
        if p['t1'] is None:
            p.update(t1=time.time(), end_s=round(self.elapsed, 1), breaks=self.brk.breaks,
                     damage=self.damage - self.done_hp, defeated=self.defeated)
        try: self.log.close()
        except Exception: pass

GIZMO_LIST_BOX = (0.215, 0.09, 0.97, 0.96)   # the Gizmo screen's list, icons to the end of the descriptions (16:9 shares)
def gizmo_list_sig(im):
    """The Gizmo list's text, small (64 wide): two views with the same signature show the same rows at the same place.
    Only the dark text counts, not the see-through backdrop behind the list (the game, which can move)."""
    W, H = im.size; u = min(W, H * 16 / 9); ux = (W - u) / 2; b = GIZMO_LIST_BOX
    c = im.crop((int(ux + u * b[0]), int(H * b[1]), int(ux + u * b[2]), int(H * b[3]))).convert('L')
    return c.point(lambda v: 255 if v < 130 else 0).resize((64, 72), Image.BOX)

def gizmo_list_same(a, b):
    """The list hasn't moved between two signatures (a few pixels' worth of difference allowed)."""
    return a is not None and b is not None and ImageStat.Stat(ImageChops.difference(a, b)).mean[0] < 1.5   # still: 0, moving: 3.7+

def sim_gate_pill(im, box, u):
    """Is the exit gate's button there: its dark blue-grey pill (the words on it), not the gate's floating label (the
    same words over the scenery)? The pill is one flat blue-grey, about (77, 76, 93) and (61, 61, 80) at its darkest,
    measured on nine frames at the gate (1 Nov): rows with that color over at least 0.04 of the 16:9 width, for a band
    about as tall as the pill (0.012 of the width). Counted, not one unbroken run, since the white letters cut its
    middle rows into pieces. Only that color: dark in general also matched a fade, a dimmed menu and the player's own
    dark armor beside the button."""
    band = im.crop(tuple(int(v) for v in box)).convert('RGB')
    w = 400; h = max(1, round(band.height * w / band.width)); band = band.resize((w, h), Image.BOX)
    need = 0.04 * u * w / (box[2] - box[0]); px = band.load(); rows = 0; best = 0   # (lettered rows: 0.053+)
    for y in range(h):
        n = 0
        for x in range(w):
            r, g, b = px[x, y]
            if 55 <= r <= 100 and 55 <= g <= 100 and b <= 125 and b - r >= 8 and b - g >= 8: n += 1
        rows = rows + 1 if n >= need else 0; best = max(best, rows)
    return best >= 0.012 * u * w / (box[2] - box[0])

class OcrWorker(threading.Thread):
    """Reads text crops on its own OCR helper so the frame loop never waits for OCR (one job at a time). Given an
    `ocr`, it reads on that one instead (names_bg: the frame loop's helper, the one with the name models)."""
    def __init__(self, ocr=None):
        super().__init__(daemon=True)
        self.ocr = ocr; self.own = ocr is None; self.job = None; self.result = None; self.wake = threading.Event(); self.stop = False
    @property
    def busy(self): return self.job is not None
    def submit(self, crops, meta, **kw):           # kw: Ocr.read's names= and raws=
        if self.job is None: self.job = (crops, meta, kw); self.wake.set()
    def take(self):
        r, self.result = self.result, None
        return r
    def run(self):
        if self.own:
            try: self.ocr = R.Ocr()                 # its helper starts here, so the meter's first look doesn't wait
            except Exception as e: log.warning(f'background OCR helper did not start: {e!r}'); return
        while not self.stop:
            if not self.wake.wait(0.5): continue
            self.wake.clear()
            if self.stop or self.job is None: continue                  # woken to close
            crops, meta, kw = self.job
            t0 = time.perf_counter()
            try: text = self.ocr.read(crops, **kw)
            except Exception as e:
                log.warning(f'background OCR failed: {e!r}'); text = {}
            self.longest = max(getattr(self, 'longest', 0.0), (time.perf_counter() - t0) * 1000)   # for the load line
            self.result = (text, meta); self.job = None
    def close(self):
        self.stop = True; self.wake.set()
        if self.ocr and self.own: self.ocr.close()

def load_korean_names(player_log=None):
    """In the background: the game's English and Korean fonts (read from the game install once, then kept in the data
    folder), so every OCR reader settles Korean player names against them (hangulnames). Until then, or when the
    fonts can't be read, names are read by the English OCR alone."""
    def run():
        try:
            import gamefont
            folder = os.path.join(HERE, 'gamefont')
            latin = gamefont.GameFont.load(folder, player_log)
            korean = gamefont.GameFont.load(folder, player_log, gamefont.KOREAN_FONT) if latin else None
            if korean: R.Ocr.korean = K.KoreanNames(latin, korean); log.info("Korean names: on (the game's Korean font)")
            else: log.info(f'Korean names: off (game font not readable: {gamefont.GameFont.error})')
        except Exception as e: log.info(f'Korean names: off ({e!r})')
    threading.Thread(target=run, daemon=True, name='korean-names').start()

# What the meter can't read without English OCR: the game's text is English, and Windows OCR reads it all but player
# names (the helper's own text models read those). The app's warning says the same (DPS Meter page, Dashboard).
NO_ENGLISH_READS = 'boss names and levels, result screens, Battle Data, the battle prep and Party screens'
OCR_RECHECK = 60        # seconds between looks for English OCR added while the meter runs (Meter.ocr_recheck)
OCR_SAY_AGAIN = 3600    # seconds before a look's warning that hasn't changed is in meter.log again (Meter.ocr_warn)

def ocr_language_note(ocr):
    """Once at start, in meter.log: the language Windows OCR reads in (the meter's own reader's, Ocr.language), with
    a warning when it isn't English, or there is none (no OCR language on the PC, or the helper didn't start: NoOcr).
    Before, a PC with no English OCR only had 'OCR helper did not start' in its log, or nothing (76c9f87), and its
    runs weren't read. English added later is found by Meter.ocr_recheck, which says so."""
    lang = ocr.language
    # Settings' page is Language on Windows 10, Language & region on 11 (adding English there installs its OCR)
    fix = ('Windows Settings > Time & language > Language (Language & region on Windows 11) > Add a language > English '
           "(United States), or Install English OCR on aLogs' DPS Meter page")
    if not ocr.started: log.warning(f"Windows OCR: none (its helper didn't start): {NO_ENGLISH_READS} can't be read, "
                                    "so runs aren't recorded or sent")
    # not said: a plain READY, which only a helper from before READY<TAB> gives (ocr_win.ps1 says its language too),
    # such as the built copies in bossbar/dist until the next meter:build
    elif lang is None: log.info('Windows OCR: language not said (an OCR helper from before this version: ocr-helper.exe '
                                'or ocr_win.ps1)')
    elif R.ocr_english(lang): log.info(f'Windows OCR: {lang}')
    elif lang: log.warning(f"Windows OCR: {lang}, not English: {NO_ENGLISH_READS} can't be read reliably, so runs may "
                           f"not be recorded or sent. Fix: {fix}")
    else: log.warning(f"Windows OCR: none (no OCR language on this PC): {NO_ENGLISH_READS} can't be read, so runs "
                      f"aren't recorded or sent. Fix: {fix}")

# ---------------------------------------------------------------- meter
class Meter:
    def __init__(self, args, out):
        self.args, self.out = args, out
        self.catalog, self.grades = R.load_catalog(), R.load_grades(); self.xn_tpl = X.load_templates()   # grades: refresh_cycles
        # the OCR helper not starting (an older one stopped when Windows had no en-US OCR; now it starts and says it has
        # no language): the meter then runs on what it reads from pixels (the boss bar, xN, the game font's numbers)
        try: self.ocr = R.Ocr()
        except Exception as e:
            log.warning(f'OCR helper did not start ({e!r}): text is not read'); self.ocr = R.NoOcr()
        self.ocr.keep_unsure = args.replay is None          # a replay reads the same whatever the PC's speed
        ocr_language_note(self.ocr); self.ocr_checked = time.time()       # ocr_recheck: its helpers just picked
        self.ocr_said = {}           # ocr_look's warnings the last look gave, and when each was logged (ocr_warn)
        self.ocr_found = False       # the last look's helper read English: the PC has it (status.json's ocr.switching)
        self.fight = None; self.hwnd = None; self.use_screen = False; self.blank = 0
        load_korean_names(getattr(args, 'player_log', None))
        self.ocr_bg = None if args.replay is not None else OcrWorker()        # the boss bar's title, timer, xN
        self.screen_bg = None if args.replay is not None else OcrWorker()     # the screens' text (check_battle_data)
        # the screens' player names, read for the first time (names_first), on self.ocr's own helper
        self.names_bg = None if args.replay is not None or not self.ocr.native else OcrWorker(self.ocr)
        for w in (self.ocr_bg, self.screen_bg, self.names_bg):
            if w: w.start()
        self.frames = 0; self.frame_size = (0, 0)
        self.bar = None; self.bar_seen = 0.0; self.last_text = 0.0; self.last_title = 0.0; self.last_frame = 0.0
        self.last_search = 0.0; self.end_sig = None
        self.frame_full = True; self.top_sig = None; self.band_frame = False    # is the last frame whole (grab); its top part's signature (step)
        self.pending_run = None      # (fight summary, deadline): a best run waiting for its Battle Data
        try:                                     # where bars were last seen (bar_spot.json), for R.bar_at
            with open(BAR_SPOT, encoding='utf-8') as f: self.bar_spots = [s for s in json.load(f) if isinstance(s, dict)]
        except (OSError, ValueError, TypeError): self.bar_spots = []
        self.switch_votes = []; self.stop = False; self.space = None; self.bossrush_space = False
        self.rush_spaces = set()     # spaces the game log called Interlink (re-entered for each of its bosses)
        self.sim_spaces = set(); self.holosim_space = False   # ...and Holo-Battle Sim (one space for a whole run)
        self.duds = []; self.dud_cand = None   # bar-like shapes that never named a boss (dud_check)
        self.hidden_drop = None      # (fill, readings in a row, last counted) of a big drop read at the locked spot
        self.events = queue.Queue(); self.logw = None; self.awaiting = None; self.last_results = 0.0; self.battle_saved = 0.0
        self.last_nobar = 0.0; self.nobar_count = 0; self.last_size = None; self.last_snap = 0.0; self.origin = (0, 0)
        self.after_fight = None      # [fight name, save end screens until, last save]
        self.catch_seen = 0.0        # last time the game's "Catch Countdown" was on screen
        self.stats_seen = 0.0; self.last_scores = {}; self.result_seen = 0.0
        self.party = None            # {'at', 'members': [{'name', 'ai'}]} from the battle prep screen
        self.icons = None            # icons.Icons: the game's head portraits and buff icons (loaded in the background)
        self.icons_state = None      # None, 'loading', 'ready', 'failed'
        self.icon_read = None        # the Party screen icon read running in the background (a thread)
        self.last_fight = None       # (Fight, ended at) - the result screen doesn't name the boss
        self.last_screen = None      # game area on screen (x, y, w, h), for placing the DPS window
        self.hold = InputBlock(log.info); self.hold_started = 0.0; self.clicks = 0; self.unread = 0; self.last_click = 0.0
        self.numbers = None             # the damage-number recorder (experimental), made when the option is on
        try: self.cfg_stamp = os.stat(C.SETTINGS_FILE).st_mtime   # the settings as loaded at start (a save during start-up was missed)
        except OSError: pass
        self.auto_until = 0.0        # automatic "View Battle Data" session running until this time
        self.last_status = 0.0; self.status_text = 'starting'
        self.submitter = None
        self.stop_handles = [] if args.replay else stop_handles()
        if not args.replay and not args.no_submit:
            self.submitter = S.Submitter(); self.submitter.on_sent = self.sent_notice; self.submitter.start()
        self.uid = None if args.replay else last_uid(args.player_log)
        # your in-game name, only ever from the game: saved for this UID (the Esc menu's is checked against the UID)
        self.character_name = known_characters().get(self.uid) if self.uid else None
        self.menu_proven = None      # (uid, name) proven from the game menu during this run of the meter
        self.link = None             # Link character in progress (link_poll)
        self.prep_boss = None        # (time, catalog row) of the boss the battle prep screen showed last
        self.rush_level = last_rush_level()   # Interlink bosses' level (the same for every boss of a season)
        self.odd_level = None        # ((puppetId, level), first and last read): a new fight's Interlink level, not rush_level
        if self.uid: log.info(f'player uid {self.uid} (from the game log)')
        if not args.replay:   # also before the game's first run: it waits for Player.log (the UID came only after a restart)
            self.logw = LogWatcher(args.player_log, self.events); self.logw.start()

    # -- capture
    def grab(self, partial=False, top_only=False, parts=None):
        """The game frame. `partial`: only its top part (TOP_PART) - enough to follow a locked boss bar, title, xN
        and timer, and noticeably cheaper at 4K. Coordinates are the same either way; frame_size is the full size.
        `partial` can also be a fraction of the height (the band down to the bar during a fight).
        `top_only`: the top part too, painted into a frame-sized image (the rest black) for the code that expects a
        whole frame; frame_full says whether the frame is whole.
        `parts`: only these boxes (shares of the frame, 0-1), painted into a frame-sized image (the rest black), for a
        screen read (screen_parts); the image's info['parts'] says so, and frame_full is True for the screen read."""
        part = (TOP_PART if partial is True else partial) if partial else (TOP_PART if top_only else None)
        if parts and self.args.replay is None: part = None
        else: parts = None
        self.frame_full = not (partial or top_only) or bool(parts)
        self.load_counts = c = getattr(self, 'load_counts', None) or {'whole': 0, 'part': 0, 'reads': 0}
        c['parts' if parts else 'whole' if part is None else 'part'] = c.get('parts' if parts else 'whole' if part is None else 'part', 0) + 1
        if self.args.replay is not None:
            im = Image.open(self.args.replay.pop(0)).convert('RGB') if self.args.replay else None
            if im is not None: self.frame_size = im.size
            self.frame_full = True
            return im
        if not self.hwnd or not ctypes.windll.user32.IsWindow(self.hwnd):
            if self.hwnd: log.warning('game window closed')
            self.hwnd = wc.find_window(self.args.title)
            if not self.hwnd: return None
            log.info(f'game window found: hwnd {self.hwnd}, client {wc.client_rect(self.hwnd)}')
            try: log.info(pcstats.system_line(self.hwnd))            # the PC behind this log, for lag reports
            except Exception as e: log.info(f'PC: not read ({e!r})')
        x, y, w, h = wc.client_rect(self.hwnd)
        self.origin = (x, y); self.frame_size = (w, h)       # where the frame sits on screen (for the HP labels)
        t0 = time.perf_counter()
        # the app's shared capture, for every module (the meter never captures by itself)
        if not wc.shared.app_present():
            if not getattr(self, 'no_app_said', False):
                self.no_app_said = True; self.out('status', 'Waiting for aLogs to share the game picture')
            return None
        im = wc.grab_parts(self.hwnd, parts) if parts else wc.grab_window(self.hwnd, part)
        self.capture_stats(t0, im, 'shared ' + (wc.shared.source or 'capture'))
        if im is None: return None
        if parts: im.info['parts'] = True
        self.use_screen = wc.shared.screen_copy        # our overlays can be in the picture: hide them from capture
        if self.use_screen: self.mask_own(im)
        if (w, h) != self.last_size:
            log.info(f'frame size {w}x{h} ({wc.shared.source})')
            self.last_size = (w, h)
        if top_only:
            canvas = getattr(self, 'canvas', None)
            if canvas is None or canvas.size != (w, h): canvas = self.canvas = Image.new('RGB', (w, h))
            canvas.paste(im, (0, 0)); return canvas
        return im

    def capture_clean(self, now=None):
        """Pictures now must be of the game alone, with no overlay in them and nothing blacked out: a proof is being
        taken (settled_frame sets proof_until). The overlay steps out of captures for that moment (under a second);
        the rest of the time the DPS window and its notes show in OBS and screenshots."""
        return (now or time.time()) < getattr(self, 'proof_until', 0)

    def mask_own(self, im):
        """The picture is of the screen (Desktop Duplication), and our overlay windows stay in screenshots and
        recordings: cover where they are (and were a moment ago, while the picture may be older) in R.COVER, so the
        meter never reads its own numbers. The bar readers look past it (the HP % label sits on the bar). The overlay
        keeps own_rects (screen pixels: x0, y0, x1, y1, until)."""
        rects = list(getattr(self, 'own_rects', None) or []) if getattr(self, 'own_in_capture', False) else []
        # Sanctum Helper's map and notes, the app's button: their spots, which they share (wincapture.overlays)
        try: rects += [(*r, 0) for r in wc.shared.overlays(skip_owner=wc.shared.OWNER_METER)]
        except Exception: pass
        if not rects: return
        now = time.time(); ox, oy = self.origin
        from PIL import ImageDraw
        d = None
        for x0, y0, x1, y1, until in list(rects):
            if until and until < now: continue
            d = d or ImageDraw.Draw(im)
            d.rectangle((x0 - ox, y0 - oy, x1 - ox - 1, y1 - oy - 1), fill=R.COVER)

    def capture_stats(self, t0, im, source):
        """Once a minute, one line on how the game is captured: where frames come from, how many arrived and how
        long getting one took here (and the app's own copy time) - so a report shows whether capture is slow."""
        st = getattr(self, 'cap_stats', None)
        now = time.time(); ms = (time.perf_counter() - t0) * 1000
        if st is None or st['source'] != source:
            log.info(f'capture: {source}')
            st = self.cap_stats = {'source': source, 'since': now, 'n': 0, 'none': 0, 'ms': 0.0, 'max': 0.0, 'app_us': 0}
        if im is None: st['none'] += 1
        else:
            st['n'] += 1; st['ms'] += ms; st['max'] = max(st['max'], ms)
            if source.startswith('shared'): st['app_us'] += wc.shared.copy_us
        if now - st['since'] >= 60:
            n = st['n'] or 1
            log.info(f"capture: {source}, {st['n'] / (now - st['since']):.1f} frames/s, {st['ms'] / n:.1f} ms each "
                     f"(max {st['max']:.0f})" + (f", the app's copy {st['app_us'] / n / 1000:.1f} ms" if st['app_us'] else '')
                     + (f", {st['none']} without a picture" if st['none'] else ''))
            self.cap_stats = {**st, 'since': now, 'n': 0, 'none': 0, 'ms': 0.0, 'max': 0.0, 'app_us': 0}
            self.load_line(now)

    def peak(self, kind, ms):
        """The longest of `kind` this minute ('look': one whole step, 'prep': preparing a screen read's crops), for the
        load line: an average hides a single long burst of work."""
        p = self.peaks = getattr(self, 'peaks', None) or {}
        p[kind] = max(p.get(kind, 0.0), ms)

    def load_line(self, now):
        """Once a minute (with the capture line): what the last minute cost - CPU for the meter, the game and the whole
        PC (% of all cores), whole frames vs top parts copied, screen text reads, and where you were. Added for lag
        reports the logs couldn't explain (B4: camera stutter at 4K)."""
        try:
            pid = ctypes.wintypes.DWORD()
            if self.hwnd: ctypes.windll.user32.GetWindowThreadProcessId(self.hwnd, ctypes.byref(pid))
            try: app = int(os.environ.get('ALOGS_PARENT') or 0)          # the aLogs app: its GPU capture runs there
            except ValueError: app = 0
            cpu = (getattr(self, 'cpu_load', None) or setattr(self, 'cpu_load', CpuLoad()) or self.cpu_load).sample(pid.value, app)
            ours = [os.getpid(), app, pcstats.sanctum_pid()]           # aLogs' programs: meter, app, Sanctum Helper
            # ...and the meter's OCR helpers (the frame loop's, which holds the name models, the bar's and the screens'
            # text, the HP number's): the name models' memory never showed in a report (278 MB for the widths 419 real
            # names need, 740 MB after 19 widths, measured on the helper alone)
            ours += [getattr(getattr(o, 'p', None), 'pid', 0) for o in (self.ocr, getattr(self.ocr_bg, 'ocr', None),
                                                                       getattr(self.screen_bg, 'ocr', None), getattr(self, 'hp_ocr', None))]
            gpu = (getattr(self, 'gpu_load', None) or setattr(self, 'gpu_load', pcstats.GpuLoad()) or self.gpu_load).sample()
            gpu_text = (f"GPU game {gpu['by_pid'].get(pid.value, 0):.0f}%, aLogs {sum(gpu['by_pid'].get(q, 0) for q in ours if q):.0f}%, "
                        f"whole card {gpu['total']:.0f}%") if gpu else 'GPU ?'
            mem = pcstats.ram()
            ours_mb = sum(m for m in (pcstats.process_mb(q) for q in ours if q) if m)
            ram_text = f"RAM {mem[0]:.1f} GB free of {mem[1]:.0f}, aLogs {ours_mb:.0f} MB" if mem else 'RAM ?'
            h = getattr(self, 'hitches', None) or [0, 0.0]; self.hitches = [0, 0.0]
            hitch_text = f"{h[0]} PC stalls (longest {h[1]:.0f} ms)" if h[0] else 'no PC stalls'
            p = getattr(self, 'peaks', None) or {}; self.peaks = {}
            bg = getattr(self, 'screen_bg', None); ocr_ms = getattr(bg, 'longest', 0.0) if bg else 0.0
            if bg: bg.longest = 0.0
            peak_text = (f"longest look {p.get('look', 0):.0f} ms, longest screen read {p.get('prep', 0):.0f} ms to prepare + "
                         f"{ocr_ms:.0f} ms to read")
            nb = getattr(self, 'names_bg', None)                       # players' names read first (names_first)
            if nb: peak_text += f", players' names {getattr(nb, 'longest', 0.0):.0f} ms to read"; nb.longest = 0.0
            c = getattr(self, 'load_counts', None) or {'whole': 0, 'part': 0, 'reads': 0}
            span = now - getattr(self, 'load_since', now - 60); self.load_since = now
            where = ('homeland' if scene_of(self.space) == HOMELAND_SCENE else 'Interlink' if self.bossrush_space
                     else 'Holo-Battle Sim' if self.holosim_space else 'world' if self.space else '?')
            fmt = lambda v: '?' if v is None else f'{v:g}%'
            log.info(f"load: CPU meter {fmt(cpu and cpu['meter'])}, aLogs app {fmt(cpu and cpu['app'])}, game {fmt(cpu and cpu['game'])}, whole PC "
                     f"{fmt(cpu and cpu['pc'])} | {c['whole'] / span:.1f} whole frames/s, {c['part'] / span:.1f} top parts/s, {c.get('parts', 0) / span:.1f} box frames/s, "
                     f"{c['reads'] / span:.1f} text reads/s | {where}, {'fight' if self.fight else 'idle' if self.idle() else 'busy'}"
                     f"{'' if not self.hwnd or wc.is_foreground(self.hwnd) else ', tabbed out'} | {gpu_text} | {ram_text} | {hitch_text} | {peak_text}")
            self.load_counts = {'whole': 0, 'part': 0, 'reads': 0}
        except Exception as e:
            log.info(f'load: not measured ({e!r})')

    def snapshot(self, now, **d):
        """Once a second: one line of what the meter currently sees."""
        if now - self.last_snap < 1.0: return
        self.last_snap = now; f = self.fight
        parts = [f'space {self.space or "?"}']
        if d.get('bar'):
            b = d['bar']; parts.append(f"bar span {b['span']:.0f}px at y {b['yc']:.0f}")
        else:
            parts.append(f'no bar ({R.LAST_FAIL[0] or "?"})')
        if 'fill' in d:
            parts.append(f"fill {d['fill'] * 100:.1f}% rgb {'%02x%02x%02x' % d['fill_rgb']} edge {'%02x%02x%02x' % d['edge_rgb']}")
        if 'xn_digits' in d: parts.append(f"xN digits {d['xn_digits']} ocr {d.get('xn_ocr')!r}")
        if 'ocr' in d: parts.append('ocr ' + json.dumps(d['ocr'], ensure_ascii=False))
        if f:
            parts.append(f'fight {f.title} Lv{f.level} seg x{f.seg} seg_fill {f.seg_fill * 100:.1f}% '
                         f'dmg {f.damage:,}/{f.max_hp:,} t {f.elapsed:.0f}s avg {f.avg:,.0f}')
        log.debug(' | '.join(parts))

    def end_fight(self, why):
        if self.numbers: self.numbers.fight_ended()
        if self.fight:
            f = self.fight
            self.fight_ended_at = time.time()   # for the leader's "ended the challenge" line (last_fight is set below)
            # The bar vanishes the moment a boss is killed or caught, before the last hit shows - but it also hides
            # during ultimates, so a vanished bar alone proves nothing. Count the rest only with confirmation: the
            # game's Catch Countdown, or the bar staying gone for the whole grace period with the boss nearly down.
            # (Interlink kills are confirmed by the result screen's time, see result_screen.)
            left = (f.max_hp - f.damage) / f.boss_hp if f.boss_hp else 1      # of the boss on the bar (Fight: max_hp is the run's)
            if why.startswith('caught') and not f.bossrush and f.hit and (f.timer is None or f.timer.predict(time.time()) is None):
                # A finishing ultimate hides the HUD (bar included) for its cutscene, then its attack plays out and the
                # killing blow lands at its end, followed by ~3 s of kill-cam and the Catch Countdown. So the fight
                # runs to the kill, 3 s before the countdown, never earlier than the last bar reading.
                # The boss kill sound, when heard, dates the blow itself (Boss kill sound, DPS Meter > Meter).
                heard = self.kill_sound()
                kill, how = ((heard, 'by its sound') if heard is not None else
                             (self.catch_seen - KILL_TO_COUNTDOWN, f'{KILL_TO_COUNTDOWN:.0f} s before the Catch Countdown'))
                if kill - f.t0 > f.elapsed:
                    log.info(f'fight {f.title}: the bar was last read at {f.elapsed:.1f}s; the kill, {how}, at {kill - f.t0:.1f}s')
                    f.elapsed = kill - f.t0
            if why.startswith('caught') or (why == 'boss bar gone' and left < 0.15):
                f.mark_defeated(why)
                log.info(f'fight {f.title}: boss defeated - final hit counted, damage set to max HP {f.max_hp:,}')
            if not f.hit: log.info(f'fight {f.title}: nobody hit it - no result shown')
            unattended = f.away > 0.5 * max(1.0, time.time() - f.created)
            if unattended: log.info(f'fight {f.title}: unattended ({f.away:.0f}s of it with the game in the background) - no result shown')
            if self.last_screen and why != 'meter closed' and f.hit and not unattended:
                try:          # the overlay's final numbers: showing them must never keep the fight from closing
                    self.out('fight_end', {'damage': f.damage, 'max_hp': f.max_hp, 'elapsed': f.elapsed, 'defeated': f.defeated,
                                           'title': f.title, 'boss': f.boss_of(),
                                           'avg': f.damage / f.elapsed if f.elapsed > 1 else 0, 'bossrush': f.bossrush,
                                           'holosim': self.holosim_space,
                                           'screen': self.last_screen})
                except Exception: log.exception('final numbers not shown')
            self.out('status', f'fight over ({why}): {f.title} Lv {f.level} - {f.damage:,} team damage in {f.elapsed:.0f}s'
                               + (' - defeated' if f.defeated else '')
                               + (f' = {f.damage / f.elapsed:,.0f} DPS' if f.elapsed > 1 else ''))
            f.ended = time.time(); f.close(); self.fight = None; self.last_fight = (f, f.ended)
            if f.elapsed > 20: self.awaiting = (f, time.time() + 60)     # look for the game's Battle Data screen
            if why != 'meter closed':                                    # keep whatever end screens follow
                self.after_fight = [os.path.splitext(os.path.basename(f.log.name))[0], time.time() + 45, 0.0]
        self.switch_votes = []

    def profile_notes(self, heads, im, now, u, ux, H):
        """Overworld notifications for the profile, kept in logs/profile_events.csv (sent with the profile later):
        - the EXP card ('Level 64', '5085/80000', 'Exp+150'), shown at the top whenever you gain EXP: level and EXP;
        - the catch card ('Irisal', 'Lv.55' over Close / View) after a Catch Countdown: what you caught."""
        events = []
        if 'level' in heads.get('pl', '').lower():
            m = re.search(r'(\d[\d,]*)\s*/\s*(\d[\d,]*)', heads.get('pe', ''))
            n = re.search(r'(\d{1,3})\s*$', self.ocr.read({'n': R.prep_labelled(
                im, (ux + u * 0.47, H * 0.148, ux + u * 0.53, H * 0.192), u / 2000, 'Level:')}).get('n', ''))
            if n and m:
                cur, need = (int(v.replace(',', '')) for v in m.groups())
                if cur <= need: self.exp_card = (getattr(self, 'exp_card', []) + [(now, int(n.group(1)), cur, need)])[-60:]   # a card left open: the last minute's reads
        ev = self.exp_settled(now)
        if ev: events.append(ev)
        c = re.match(r"\s*([A-Za-z][A-Za-z '-]{1,30}?)\s*/\s*L[vVyY]\.?\s*(\d{1,3})", heads.get('pc', ''))
        if c and re.search(r'view|close', heads.get('pb', ''), re.I):
            events.append(('catch', c.group(1).strip(), int(c.group(2)), '', ''))
        # One note per card: what counts is the level and EXP progress, so the same numbers are noted once (the
        # card's "+N" slides in a moment later and doesn't make it a new card).
        seen = getattr(self, 'profile_seen', {})
        for ev in events:
            key = ev[:4]
            if now - seen.get(key, -1e9) >= 60: self.profile_note(ev)
            seen[key] = now
            # the level and EXP progress go on the profile, beside the character (exp_settled only lets forward
            # progress through, and a waiting send is replaced by the newer one, so a run of EXP gains sends once)
            if ev[0] == 'exp' and self.submitter and self.uid and getattr(self, 'level_sent', None) != (self.uid, *ev[1:4]):
                self.level_sent = (self.uid, *ev[1:4])
                server = S.game_server()
                self.submitter.character_stats(self.uid, {'level': int(ev[1]), 'exp': ev[2], 'expNeeded': ev[3],
                                                          **({'server': server} if server else {})})
        self.profile_seen = {k: t for k, t in seen.items() if now - t < 120}

    def exp_settled(self, now):
        """The EXP card's level and current EXP, once the card is gone (no read for 2 s): the card slides in and counts
        up, so its reads come in cut off or short ('985' for 9,885) but never too high. The card's real numbers are
        then its highest read, at the level most reads agree on. Kept only when it's not behind the last noted value
        for this character (EXP never drops within a level, and a level only goes up - a new level needs two reads)."""
        card = getattr(self, 'exp_card', [])
        if not card or now - card[-1][0] < 2: return None
        self.exp_card = []
        level, votes = collections.Counter(r[1] for r in card).most_common(1)[0]
        _, _, cur, need = max((r for r in card if r[1] == level), key=lambda r: r[2])
        uid = self.uid or ''; known = getattr(self, 'exp_last', None)
        if known is None:
            known = self.exp_last = {}
            try:     # the last noted values, from earlier sessions
                with open(os.path.join(HERE, 'logs', 'profile_events.csv'), encoding='utf-8') as fh:
                    for r in csv.reader(fh):
                        if len(r) > 5 and r[2] == 'exp' and r[3].isdigit() and r[4].isdigit():   # the highest (older
                            known[r[1]] = max(known.get(r[1], (0, 0)), (int(r[3]), int(r[4])))  # notes had misreads)
            except OSError: pass
        last = known.get(uid)
        if last and (level < last[0] or (level > last[0] and votes < 2) or (level == last[0] and cur <= last[1])):
            return None
        known[uid] = (level, cur)
        return ('exp', str(level), cur, need, '')

    def profile_note(self, ev):
        kind = ev[0]
        log.info(f'profile: {kind} ' + (f'- level {ev[1]}, EXP {ev[2]:,} / {ev[3]:,}' + (f' (+{ev[4]})' if ev[4] != '' else '')
                                         if kind == 'exp' else f'- {ev[1]} Lv {ev[2]}'))
        path = os.path.join(HERE, 'logs', 'profile_events.csv'); new = not os.path.exists(path)
        try:
            with open(path, 'a', newline='', encoding='utf-8') as fh:
                w = csv.writer(fh)
                if new: w.writerow(['when', 'uid', 'kind', 'name_or_level', 'level_or_exp', 'exp_needed', 'exp_gained'])
                w.writerow([time.strftime('%Y-%m-%d %H:%M:%S'), self.uid or '', *ev])
        except OSError: pass

    def time_check(self, f, game_dps, run=None):
        """Which fight time the game's Battle Data uses: the time its team DPS implies (damage / DPS) next to the meter's
        two candidates - up to the last frame with the boss bar (what the meter shows now), and up to the fight's end
        (the bar can be hidden by a finishing ultimate). One line per fight in logs/time_check.csv, to settle it.
        run (run_of): the run's boss and damage. A two-boss middle fight's are both bosses', its first boss at its max
        HP even when the meter joined at the second one, under the stage's first boss; a one-boss fight's are its own."""
        damage, title = (run['damage'], run['title']) if run else (f.damage, f.title)
        if not game_dps or not damage: return
        game_s = damage / game_dps; to_end = getattr(f, 'ended', time.time()) - f.t0
        log.info(f"time check: the game's team DPS implies {game_s:.1f}s; the meter counted {f.elapsed:.1f}s "
                 f'(to the last bar reading) and {to_end:.1f}s (to the end of the fight)')
        path = os.path.join(HERE, 'logs', 'time_check.csv'); new = not os.path.exists(path)
        try:
            with open(path, 'a', newline='', encoding='utf-8') as fh:
                w = csv.writer(fh)
                if new: w.writerow(['when', 'boss', 'interlink', 'damage', 'game_team_dps', 'game_seconds', 'meter_seconds', 'to_end_seconds'])
                w.writerow([time.strftime('%Y-%m-%d %H:%M:%S'), title, f.bossrush, damage, game_dps,
                            round(game_s, 1), round(f.elapsed, 1), round(to_end, 1)])
        except OSError: pass

    # The screens' text boxes: (name, preparation, box as fractions of the 16:9 UI area: x0, y0, x1, y1)
    SCREEN_BOXES = (('h', 'text', (0.065, 0.035, 0.17, 0.08)), ('cs', 'text', (0.35, 0.075, 0.65, 0.155)),
                    ('bs', 'text', (0.785, 0.03, 0.97, 0.09)), ('lbl', 'text', (0.2, 0.425, 0.83, 0.47)),
                    ('cc', 'text', (0.38, 0.07, 0.62, 0.125)), ('me', 'text', (0.657, 0.068, 0.86, 0.104)),
                    ('uid', 'text', (0.657, 0.104, 0.745, 0.134)), ('eh', 'bright', (0.03, 0.08, 0.52, 0.17)),
                    # profile notes (overworld): the EXP card at the top and the catch card on the left
                    ('pl', 'bright', (0.455, 0.118, 0.545, 0.195)), ('pe', 'bright', (0.44, 0.222, 0.56, 0.278)),
                    ('pc', 'bright', (0.055, 0.485, 0.16, 0.555)), ('pb', 'bright', (0.05, 0.555, 0.17, 0.595)),
                    # Egg Heist's rank screen after a match: "1337/1350" right of the badge (then "+18" next to it)
                    ('ep', 'text', (0.72, 0.555, 0.83, 0.605)),
                    # ...and at Egg Emperor, which has no points bar: the egg count "x57" under the badge
                    ('es', 'text', (0.465, 0.73, 0.55, 0.80)),
                    # Holo-Battle Sim's Victory screen: "Battle time 00:08:22 Training of Water - Hard" under the title
                    # (bright: over a light sky the white-text cut read 'Battr Itimer', 1 Nov; sim_result reads it so)
                    ('sv', 'bright', (0.36, 0.198, 0.65, 0.233)))
    QUICK_BOXES = ('h', 'cs', 'lbl', 'cc')   # while a fight's bar is hidden: Catch Countdown and the result screens
    # The boxes that can't show where you are (the game log says where): left out of a screen read, which then does a
    # third of the work. The header ('h') and the menu ('me', 'uid') can show anywhere and are always read. Every
    # FULL_READ_S, and wherever the game log hasn't said, every box is read anyway, so a screen that turns up
    # somewhere unexpected is still caught within seconds.
    #   cs/lbl: Interlink's result screen · bs: Interlink's battle prep (the lobby, out in the world too)
    #   cc: Catch Countdown · pl/pe: the EXP card · pc/pb: the catch card · eh/ep/es: Egg Heist's end and rank screens
    #   sv: Holo-Battle Sim's Victory line
    PLACE_SKIP = {'interlink': {'cc', 'pl', 'pe', 'pc', 'pb', 'eh', 'ep', 'es', 'sv'},
                  'holo-sim': {'cs', 'lbl', 'bs', 'cc', 'pc', 'pb', 'eh', 'ep', 'es'},
                  'egg heist': {'cs', 'lbl', 'bs', 'cc', 'sv', 'pc', 'pb'},
                  'world': {'cs', 'lbl', 'sv'}}
    FULL_READ_S = 5.0

    def place(self, now):
        """Where the game log has you, for PLACE_SKIP: 'interlink', 'holo-sim', 'egg heist', 'world', or None (not known)."""
        if self.args.replay is not None or not self.space: return None
        if self.bossrush_space: return 'interlink'
        if self.holosim_space: return 'holo-sim'
        if self.sanctum_in_run(now): return 'egg heist'
        return 'world'

    def screen_crops(self, im, quick):
        """The boxes to read, prepared for OCR. A white-text box whose prepared crop has almost no black pixels is
        left out: OCR only ever sees that crop, and a single letter at its size is well over INK_MIN pixels, so
        there's nothing in it to read (reads as '', like before, at no cost)."""
        W, H = im.size; u = min(W, H * 16 / 9); ux = (W - u) / 2
        prep = {'text': R.prep_text, 'bright': R.prep_bright}; out = {}
        now = time.time(); skip = set()
        if not quick and now - getattr(self, 'full_read_at', 0) < self.FULL_READ_S:
            skip = self.PLACE_SKIP.get(self.place(now), set())
        elif not quick: self.full_read_at = now                    # this one reads every box
        # In a Holo-Battle Sim the Victory line too: a won last fight stays open while its bar is hidden (up to
        # BAR_GONE_GRACE), and only these quick boxes are read then - a player who left the Victory screen within 18 s
        # had the win never read nor sent (3 Nov)
        quick_boxes = self.QUICK_BOXES + (('sv',) if self.holosim_space else ())
        for name, kind, (x0, y0, x1, y1) in self.SCREEN_BOXES:
            if quick and name not in quick_boxes: continue
            if name in skip: continue
            crop = prep[kind](im, (ux + u * x0, H * y0, ux + u * x1, H * y1), u / 2000)
            if kind == 'text' and sum(crop.histogram()[:128]) < INK_MIN: continue
            out[name] = crop
        return out

    # Battle Data's four values on each card (dark digits on a light card, right-aligned after their label): the
    # rows' heights, as shares of the screen; the value's box runs from the card's centre + 0.015 to + 0.081 (u)
    BD_ROWS = (('avg_dps', 0.5833), ('break', 0.6255), ('taken', 0.6667), ('heal', 0.7088))

    @staticmethod
    def bd_card_count(im, u, ux, H):
        """How many cards Battle Data shows, counted from pixels: the light runs along a line between the BREAK and
        Damage Taken rows (y 0.645), each about a card wide (0.165-0.167 u on 55 real 4K captures) and centred where
        that many cards side by side would be (0.1943 u apart, as their boxes are placed). 0 when they don't line up
        like that (the screen sliding in, something in front of it). OCR's count put every box half a card off when it
        missed one: its first read found 3 of 4 cards on 14 of those 55 screens and none on 5 more; this found 4 on all
        55, scaled down to 720p as well (under 1 ms)."""
        y = int(H * 0.645); N = 400
        row = im.crop((int(ux), y, int(ux + u), y + 1)).convert('RGB').resize((N, 1), Image.BOX).tobytes()
        runs, start = [], None
        for x in range(N + 1):
            px = row[3 * x:3 * x + 3] if x < N else b'\0\0\0'
            light = min(px) >= 190 and max(px) - min(px) <= 40           # the card's pale grey, not the coloured scene
            if light and start is None: start = x
            elif not light and start is not None: runs.append((start / N, x / N)); start = None
        cards = [(a, b) for a, b in runs if b - a >= 0.12]
        n = len(cards)
        if not 1 <= n <= 4: return 0
        if any(b - a > 0.19 or abs((a + b) / 2 - (0.5 + (i - (n - 1) / 2) * 0.1943)) > 0.02 for i, (a, b) in enumerate(cards)):
            return 0
        return n

    def battle_data_font(self, im, u, ux, H, read):
        """Each card's values read digit by digit against the game's own digits, over OCR's in `read` (in place).
        OCR skipped 7-digit BREAK values, which moved the card's later numbers up a row (BREAK got Damage Taken's,
        and so on: 23 cards on 1 Nov). A card where a sure read disagrees with OCR keeps only the sure reads; its other
        values are left unread rather than sent from the wrong row."""
        reader = self.name_reader()
        if not reader: return
        n = len(read)
        for i, card in enumerate(read):
            c = ux + u * (0.5 + (i - (n - 1) / 2) * 0.1943)
            sure = {key: reader.number(im, (c + u * 0.015, H * (y - 0.016), c + u * 0.081, H * (y + 0.016)), dark=True, right=True)
                    for key, y in self.BD_ROWS}
            sure = {k: v for k, v in sure.items() if v is not None}
            slid = any(card.get(k) != v for k, v in sure.items())
            if slid: log.info(f'Battle Data card {i + 1}: OCR {card} disagrees with the game font {sure}: the font kept')
            for key, _ in self.BD_ROWS:
                card[key] = sure[key] if key in sure else (None if slid else card.get(key))

    def check_battle_data(self, im, now):
        """The game's screens with text: Battle Data, result screens, Catch Countdown, prep, Egg Heist, profile cards.
        Live, the text is read on its own thread (screen_bg) so a frame never waits for it: a new read starts as
        0.1 s after the last one (~40 ms) when the screen changed, about every 1.5 s when it didn't, and back to back -
        only the few boxes that matter then - while a fight's bar is hidden (the Catch Countdown dates the kill, see
        end_fight) or the automatic Battle Data click waits for its screen. What it read is handled here with the
        frame and time it was read from."""
        quick = self.auto_active() or (self.fight is not None and now - self.bar_seen > 0.5)
        bg = getattr(self, 'screen_bg', None)
        if bg is None:                                                  # replays and tests: read in place
            if now - self.last_results < (0.5 if quick else 1.5): return
            self.last_results = now
            crops = self.screen_crops(im, False)
            return self.read_screen(im, now, self.ocr.read(crops) if crops else {})
        if self.names_waiting(): return             # a screen waits for its players' names: it's handled first
        done = bg.take()
        if done: self.read_screen(done[1][0], done[1][1], done[0])
        if bg.busy or self.names_waiting() or not getattr(self, 'frame_full', True) or not self.screen_read_due(now, quick): return
        self.last_results = now; self.screen_read_sig = getattr(self, "top_sig", None)
        t0 = time.perf_counter()
        crops = self.screen_crops(im, quick)
        self.peak('prep', (time.perf_counter() - t0) * 1000)
        if crops:
            bg.submit(crops, (im, now))
            if getattr(self, 'load_counts', None): self.load_counts['reads'] += 1
        else: self.read_screen(im, now, {})                            # nothing on screen to read

    def names_first(self, handler, args, crops=None, pad=0, raws=None):
        """handler(*args, names=(crops, raws)): a screen's reader, whose read takes these player-name boxes (crops:
        {key: prepared box}, `pad` their padding, raws: grey_crop's, for Ocr.read). A name read the first time costs
        the two text models ~0.2 s, and the frame loop stood still for them. So when a box isn't in the name cache,
        names_bg reads the boxes first, on the same helper, and the reader runs once they're in (names_due) with its
        own frame and time: its read then takes the names from the cache, so it reads what it would have. No other
        screen is read meanwhile (names_waiting), so screens are still handled in order. At once when every box was
        read before (the same screen read again), and in replays and tests. On 12 real 4K screens of each, a first
        read held the frame loop 0.9 s (result, Party) and 1.1 s (Battle Data) at the median, 1.7 s at most; now
        0.21, 0.05 and 0.22 s, 0.24 s at most, with the same readings."""
        nb = getattr(self, 'names_bg', None)
        names = {k: pad for k in crops or {}}
        if crops and nb is not None and not nb.busy and not self.ocr.cached(crops, names):
            nb.submit(crops, (handler, args, (crops, raws)), names=names, raws=raws); return
        handler(*args, names=(crops, raws) if crops else None)

    def names_due(self):
        """The screen reader waiting for its players' names (names_first), once names_bg has read them."""
        done = self.names_bg.take() if getattr(self, 'names_bg', None) else None
        if done:
            handler, args, names = done[1]
            handler(*args, names=names)

    def names_waiting(self):
        nb = getattr(self, 'names_bg', None)
        return bool(nb and (nb.busy or nb.result is not None))

    def screen_read_due(self, now, quick=False):
        """Time for the next screen read: 0.1 s after the last one when the screen changed since (seen on the top part
        of the frame, which every look has), about every 1.5 s when it didn't, at once in quick mode."""
        last, cur = getattr(self, 'screen_read_sig', None), getattr(self, 'top_sig', None)
        changed = last is None or cur is None or screen_changed(cur, last, limit=6)
        # idle out in the world, turning the camera changes every frame: a whole 4K copy and ~17 text reads every look
        # made the camera stutter on a player's PC (B4). Once a second is enough for the screens read there.
        gap = 0.0 if quick else 0.1 if changed else 1.5
        # a fight's bar hidden: 4 reads a second for the Catch Countdown and end screens, not back to back (B5: 9-13 a
        # second at 4K); the automatic Battle Data click still reads at once
        if quick and not self.auto_active(): gap = 0.25
        if not quick and self.idle(): gap = max(gap, 1.0)
        return now - self.last_results >= gap

    def full_frame_due(self, now):
        """Between fights only the frame's top part is copied (where a boss bar appears; ~3x cheaper at 4K) unless
        the whole frame is needed: for a screen read, the end-screen note, Egg Heist's fade or diagnostic frames."""
        if self.end_sig is not None or getattr(self, 'egg_hold', None) or getattr(self, 'sim_tour', None) or self.sanctum_in_run(now): return True
        if getattr(self, 'sim_ctl', None) or (self.holosim_space and (getattr(self, 'sim_gate_state', None) or {}).get('last')): return True
        cap = self.args.cfg['capture']; a = self.after_fight
        if a and cap['end_screens'] and now - a[2] >= 3: return True
        if cap['nobar_frames'] and self.bossrush_space and now - self.last_nobar >= 5 and self.nobar_count < 30: return True
        bg = getattr(self, 'screen_bg', None)
        if bg is None: return True
        if not bg.busy and not self.names_waiting() and self.screen_read_due(now):
            # only for a screen read: just its boxes (screen_parts), unless a screen is up (then whole frames until it's gone)
            return True if now < getattr(self, 'full_until', 0) else 'screen'
        return False

    FULL_HOLD_S = 3.0      # whole frames for this long after a screen read saw text (B8)

    def screen_parts(self, now):
        """The boxes a screen read needs, as shares of the frame: the top part (where a boss bar shows; it's also
        where most screens' titles are) and the boxes below it that this read looks at (screen_crops). About half of a
        whole frame at 4K; nothing else leaves the graphics card. A screen found in them gets whole frames (read_screen)."""
        W, H = self.frame_size
        if not W or not H: return None
        u = min(W, H * 16 / 9); ux = (W - u) / 2; pad = 0.004
        skip = self.PLACE_SKIP.get(self.place(now), set()) if now - getattr(self, 'full_read_at', 0) < self.FULL_READ_S else set()
        out = [(0.0, 0.0, 1.0, TOP_PART)]
        for name, kind, (x0, y0, x1, y1) in self.SCREEN_BOXES:
            if name in skip or y1 <= TOP_PART: continue
            out.append((max(0.0, (ux + u * x0) / W - pad), max(0.0, y0 - pad), min(1.0, (ux + u * x1) / W + pad), min(1.0, y1 + pad)))
        return out

    @staticmethod
    def screen_spotted(heads):
        """Which screen read_screen would act on (its own tests, roughly), or None: the cue for whole frames (B8)."""
        g = lambda k: (heads.get(k) or '').lower()
        head = g('h'); hl = re.sub(r'[^a-z]', '', head)
        tests = (('result', 'score' in g('lbl') and 'time' in g('lbl')), ('Holo-Battle Sim result', 'battle time' in g('sv')),
                 ('challenge', 'challenge' in g('cs')), ('battle prep', 'score' in g('bs')), ('Catch Countdown', 'catch' in g('cc')),
                 ('Egg Heist rank', 'rank info' in head), ('profile', head.strip().startswith('profile')),
                 ('party', head.strip().startswith('party')), ('Holo-Battle', hl.startswith('holobattlei')),
                 ('Egg Heist', 'adventure' in g('eh') or re.fullmatch(r'\s*\d{1,6}\s*/\s*\d{1,6}\s*', g('ep')) is not None
                  or re.fullmatch(r'\s*[x×]\s*\d{1,6}\s*', g('es')) is not None),
                 ('menu', bool(g('me') and g('uid'))), ('Battle Data', 'battle' in hl and 'dat' in hl),
                 ('EXP card', 'level' in g('pl')), ('catch card', bool(re.search(r'view|close', g('pb'))) and '/' in g('pc')))
        return next((name for name, hit in tests if hit), None)

    def read_screen(self, im, now, heads):
        self.screens_gone(now)
        # a screen the meter reads is up: its readers look at more of the frame than the boxes. A frame of only the
        # boxes (screen_parts) is read again whole at once; whole frames go on until FULL_HOLD_S after the last one
        spotted = self.screen_spotted(heads)
        if spotted and self.args.replay is None:
            self.full_until = now + self.FULL_HOLD_S
            if im.info.get('parts'):
                if now - getattr(self, 'parts_said', 0) > 60:
                    self.parts_said = now; log.info(f"screen read: {spotted} spotted, whole frames while it's up")
                self.last_results = 0; return
        W, H = im.size; u = min(W, H * 16 / 9); ux = (W - u) / 2
        self.menu_name(im, u, ux, H, heads)
        self.profile_notes(heads, im, now, u, ux, H)
        if 'catch' in heads.get('cc', '').lower():                     # a caught boss: its fight was won
            if now - self.catch_seen > 30: log.info(f"Catch Countdown on screen: {heads.get('cc')!r}")
            self.catch_seen = now
        is_result = 'score' in heads.get('lbl', '').lower() and 'time' in heads.get('lbl', '').lower()
        # 'Battle Data' itself: 'Battle Buffs' and 'Holo-Battle I/II' share the word (21 of 29 detections were those,
        # which ended fights and replaced a run's Battle Data with an empty one)
        head = heads.get('h', ''); hl = re.sub(r'[^a-z]', '', head.lower())
        is_bd = 'battle' in hl and 'dat' in hl and 'holo' not in hl and 'buff' not in hl
        if self.auto_active() and not is_result and not is_bd and now - self.hold_started > 4:
            self.end_auto('screen changed')
        if is_result:
            if (now - self.result_seen >= 30 and not self.args.no_auto_click and self.args.replay is None
                    and self.hwnd and wc.is_foreground(self.hwnd)):
                self.start_auto(now)                                   # a new result screen: before anything slow
            # click first (the screen's numbers are read from this frame either way); the button may not take the
            # first click while the screen is still sliding in, so retry every second
            if self.auto_active() and now - self.last_click > 1.0:
                if self.clicks < 6: self.open_battle_data(im, u, ux, H)                 # first click or a retry
                else: self.end_auto('Battle Data did not open after 6 clicks')
            self.test_frame(im, 'result', now)
            crops, grey = self.result_names(im, u, ux, H)
            self.names_first(self.result_screen, (im, now, u, ux, H, heads.get('cs', '')), crops, 24, grey)
            return
        if 'battle time' in heads.get('sv', '').lower(): self.test_frame(im, 'sim_result', now); self.sim_result(im, now, u, ux, H); return
        if 'challenge' in heads.get('cs', '').lower(): self.test_frame(im, 'challenge', now); self.challenge_stats(im, now, u, ux, H); return
        if 'score' in heads.get('bs', '').lower(): self.test_frame(im, 'prep', now); self.prep_screen(im, now, u, ux, H, heads['bs']); return
        if 'rank info' in head.lower(): self.test_frame(im, 'egg_rank', now); self.egg_rank_info(im, now, u, ux, H); return
        if head.strip().lower().startswith('profile'): self.test_frame(im, 'profile', now); self.profile_screen(im, now, u, ux, H); return
        if head.strip().lower().startswith('party'):
            self.test_frame(im, 'party', now)
            # at party_screen's own pace (2 s): a pill's ready mark or voice icon changes its look, and each change
            # would send all four to the name models at every screen read, where the screen reads them every 2 s
            if now - getattr(self, 'party_seen', 0) >= 2:
                self.names_first(self.party_screen, (im, now, u, ux, H), self.party_names(im, u, ux, H), 20)
            return
        if re.sub(r'[^a-z]', '', head.lower()).startswith('holobattlei'): self.interlink_screen(im, now, u, ux, H); return
        if re.fullmatch(r'\s*\d{1,6}\s*/\s*\d{1,6}\s*', heads.get('ep', '')): self.egg_rank_gain(im, now, u, ux, H, heads['ep'])
        elif re.fullmatch(r'\s*[xX×]\s*\d{1,6}\s*', heads.get('es', '')): self.egg_emperor_count(im, now, u, ux, H, heads['es'])
        if 'adventure' in heads.get('eh', '').lower(): self.test_frame(im, 'egg_result', now); self.egg_result(im, now, u, ux, H, heads['eh']); return
        if heads.get('me') and heads.get('uid'): self.test_frame(im, 'menu', now)
        if not is_bd: return
        self.test_frame(im, 'battle_data', now)
        # Battle Data is open: keep the inputs held BATTLE_DATA_PAUSE more seconds while it's read again and again
        if self.auto_until and not getattr(self, 'bd_hold', False):
            self.bd_hold = True; self.auto_until = min(self.auto_until, now + BATTLE_DATA_PAUSE)
            log.info(f'Battle Data is open: holding {BATTLE_DATA_PAUSE} s while it is read')
        crops, grey = self.bd_names(im, self.bd_card_count(im, u, ux, H), u, ux, H)
        self.names_first(self.battle_data, (im, now, u, ux, H, head), crops, 0, grey)

    @staticmethod
    def bd_names(im, n, u, ux, H):
        """Battle Data's name tabs of n cards: prep_white's for Windows OCR and the Korean model, grey_crop's for the
        Latin model ({card: box} each)."""
        boxes = {i: (ux + u * (0.5 + (i - (n - 1) / 2) * 0.1943 - 0.045), H * 0.512,
                     ux + u * (0.5 + (i - (n - 1) / 2) * 0.1943 + 0.045), H * 0.547) for i in range(n)}
        return {i: R.prep_white(im, b, u / 2000) for i, b in boxes.items()}, {i: R.grey_crop(im, b) for i, b in boxes.items()}

    def battle_data(self, im, now, u, ux, H, head, names=None):
        """Battle Data on screen: each card's four values and its player, voted over the reads of one appearance, saved
        with the fight. `names`: bd_names' boxes, already read (names_first)."""
        if self.fight: self.end_fight('Battle Data screen')           # can show up while the bar dropout is still bridged
        # one appearance is one Battle Data (left open, or reopened from a menu, it isn't a new one for 30 s), read again
        # while it's up (SCREEN_READS in all, 0.5 s apart): each card value is the one most reads agree on
        shown_already = now - getattr(self, 'bd_seen', 0) < 6; self.bd_seen = now
        # fewer cards than the Party screen's players: read on (up to twice as often) rather than settle on them
        party = self.party_for_result(now) or []
        budget = SCREEN_READS * (2 if len(party) > getattr(self, 'bd_cards', 0) else 1)
        again = (shown_already and now - self.battle_saved < 30 and getattr(self, 'bd_reads', 0) < budget
                 and now - getattr(self, 'bd_last', 0) >= 0.5)
        if (shown_already or now - self.battle_saved < 30) and not again: return
        self.bd_last = now
        if not again:
            self.battle_saved = now; self.bd_reads = 0; self.bd_votes = {}; self.bd_shot = None; self.bd_cards = 0
            log.info(f'Battle Data screen detected (header OCR {head!r})')
        self.bd_reads += 1
        # the four stat rows of all cards (dark text on light cards)
        rows = im.crop((int(ux + u * 0.125), int(H * 0.564), int(ux + u * 0.875), int(H * 0.73))).convert('L')
        # smooth (LANCZOS) enlarging keeps the small 720p digits apart, so no card goes missing
        k = max(1.0, 2 * 2000 / u); rows = rows.resize((int(rows.width * k), int(rows.height * k)), Image.LANCZOS)
        text = self.ocr.read({'d': ImageOps.expand(rows, border=20, fill=255)}).get('d', '')
        read = parse_battle_data(text)
        log.info(f'Battle Data read {self.bd_reads}: OCR {text!r} -> {read}')   # whole: a run can be re-read from the log
        # the cards are counted on screen (bd_card_count), by OCR when they can't be or it found more. Either way the
        # count is the most any read found: a read never finds a card that isn't there (OCR needs each card's 'Average
        # DPS'), and settling on the commonest let one copy of a 4-player run keep 3 cards (31 Oct). A count on screen
        # under OCR's is cards it didn't see (the middle two of four sit where two cards would), not fewer cards
        before = getattr(self, 'bd_cards', 0); seen = self.bd_card_count(im, u, ux, H)
        n = self.bd_cards = min(4, max(before, seen, len(read)))
        read = [dict(read[i]) if i < len(read) else {} for i in range(n)]
        self.battle_data_font(im, u, ux, H, read)
        # the count and this read's numbers at it. A count off the party's is only logged: the game's panel
        # (Guis/Panels/BossRushBattleDetailResult) lists the run's player records, AI ones too, but where the cards sit
        # is in its prefab, and no 2- or 3-card screen has been seen yet
        if n != before:
            said = ('counted on screen' if seen >= len(read) else f"OCR's count (only {seen} counted on screen)") if seen \
                else "OCR's count (not counted on screen)"
            if party and len(party) != n: said += f', but the Party screen had {len(party)} players'
            log.info(f'Battle Data cards: {said} -> {n} cards {[{k: c.get(k) for k, _ in self.BD_ROWS} for c in read]}')
        cards = [{k: vote(self.bd_votes, (i, k), read[i].get(k)) for k in ('avg_dps', 'break', 'taken', 'heal')}
                 for i in range(n)]
        # each card's player: the name tab under the portrait (cards sit side by side, centred, in slot order 1P-4P),
        # then the Party screen's player of that slot (party_slots), so a card is tied to its player, not its position.
        # The Latin model reads the tab as it is on screen (grey_crop), Windows OCR and the Korean model prep_white's
        crops, grey = names if names and len(names[0]) == n else self.bd_names(im, n, u, ux, H)
        names = self.ocr.read(crops, names={i: 0 for i in crops}, raws=grey) if n else {}
        for i, c in enumerate(cards):
            c.update(slot=i, name=vote(self.bd_votes, (i, 'name'), (names.get(i) or '').strip(' /|') or None) or '')
        party_slots(cards, party, sure={i for i in crops if sure_read(self.ocr, crops[i])},
                    you=self.character_name if character_verified(self.uid) else None)
        vals = [c['avg_dps'] for c in cards if c['avg_dps'] is not None]
        lf = self.last_fight if self.last_fight and now - self.last_fight[1] < 180 else None
        if again:                                                  # a later read: only the agreed numbers change
            if lf: self.fight_summary(lf[0], battle_data={'players': cards, 'team_avg_dps': sum(vals) if vals else None,
                                                          'screenshot': self.bd_shot, 'reads': self.bd_reads})
            return
        stamp = time.strftime('%Y%m%d_%H%M%S'); out_dir = os.path.join(HERE, 'logs', 'battle_data'); os.makedirs(out_dir, exist_ok=True)
        shot = self.picture(im, 'battle_data', stamp, proof=bool(lf and self.run_to_send(lf[0]))); self.bd_shot = os.path.relpath(shot, HERE) if shot else None
        msg = f'Battle Data screen saved: {os.path.relpath(shot, HERE) if shot else "numbers only (no picture kept)"}'
        if vals: msg += f' - game team DPS {sum(vals):,} (players {", ".join(f"{v:,}" for v in vals)})'
        f = self.awaiting[0] if self.awaiting and now <= self.awaiting[1] else None
        if f and vals:
            game_dps = sum(vals); T = f.elapsed; loss = f.damage / f.max_hp if f.max_hp else 0
            run = self.run_of(f)                                       # the run's damage: a two-boss fight's both bosses
            msg += f' vs meter {run["damage"] / T:,.0f} ({run["title"]} Lv {f.level})' if T > 1 else ''
            self.time_check(f, game_dps, run)
            calibrate = f.bossrush and f.started_at_top and loss > 0.02 and T > 60
            if calibrate and run['bosses'] is not None:
                # Never from a two-boss middle fight, whichever boss it ended on: Battle Data has one team DPS for the
                # run (BossRushBattleDetailResult; whether it covers both bosses isn't known until a live run shows it),
                # and no share of it is one boss's. Taken as the boss on the bar's, it put the second boss's max HP
                # ~1.2x too high, inside the guard below, and kept that for the boss from then on (calibration.json)
                calibrate = False
                log.info(f"Battle Data: no max HP measured from a two-boss fight ({', '.join(b['name'] for b in run['bosses'])}): "
                         "its team DPS is the run's, not one boss's")
            if calibrate:
                measured = game_dps * T / loss
                predicted = R.boss_max_hp(f.row, f.level, use_calibration=False)
            if calibrate and predicted and 0.5 <= measured / predicted <= 2:
                # far off the prediction means the wrong boss was matched, not a better HP figure: never save that
                kept = R.save_calibration(f.row['puppetId'], f.level, measured, predicted,
                                          f'{time.strftime("%Y-%m-%d %H:%M")} game {game_dps} T {T:.0f}')
                msg += f' | max HP measured {measured:,.0f} (predicted {predicted:,}); using {kept:,} from now on'
            self.awaiting = None
        self.out('status', msg)
        self.end_sig = screen_sig(im)
        if lf:
            self.fight_summary(lf[0], battle_data={'players': cards, 'team_avg_dps': sum(vals) if vals else None,
                                                   'screenshot': os.path.relpath(shot, HERE) if shot else None})
        with open(os.path.join(out_dir, stamp + '.txt'), 'w', encoding='utf-8') as fh:
            fh.write(msg + '\n\n')
            for i, c in enumerate(cards, 1):
                fh.write(f"player {i}: average DPS {c['avg_dps']}, BREAK {c['break']}, damage taken {c['taken']}, heal {c['heal']}\n")
            fh.write('\nraw OCR of the stat rows:\n' + text.replace(' / ', '\n') + '\n')
            if f: fh.write(f'\nmeter fight log: {os.path.basename(f.log.name)}\n')

    def sanctum_in_run(self, now):
        """Sanctum Helper's live status says an Egg Heist run is on (it watches the run's map). Read about once a
        second; remembers when a run was last seen, since the run ends right as the screen fades."""
        if now - getattr(self, 'sh_checked', 0) >= 1:
            self.sh_checked = now
            path = os.path.join(os.environ.get('LOCALAPPDATA', ''), 'SanctumHelper', 'hosted', 'status.json')
            try:
                with open(path, encoding='utf-8') as fh: st = json.load(fh)
                if st.get('inRun') and time.time() * 1000 - float(st.get('time') or 0) < 5000: self.sh_run_seen = now
            except (OSError, ValueError, TypeError): pass
        return now - getattr(self, 'sh_run_seen', 0) < 30

    def egg_fade(self, im, now):
        """Egg Heist (opt-in, experimental): the screen fades to black right before the end screen, and a stray
        press there skips it. From the fade until the end screen is read (egg_result), keyboard and mouse presses are
        held back, and the controller hidden when that is switched on too. A black screen that isn't the end (loading)
        lets go 5 s after the picture comes back; the time limit and Ctrl+Shift+F12 always end it. It starts once the
        screen has stayed black for EGG_BLACK, so a dark flash or a camera cut doesn't block anything.
        Only a fade inside a live run counts: the end screen shows in the match's own space and the game log's
        leave/enter only comes after it is closed (28 Oct matches), while every false hold (30 Oct) was a load: right
        after leaving the island, in the Sanctum up to 30 s after the run, or 3 s into the next island's load-in."""
        r = self.args.cfg['results']
        hold = getattr(self, 'egg_hold', None)              # [ends at, picture came back at, space it started in]
        if self.space != getattr(self, 'egg_space', None): self.egg_space = self.space; self.egg_space_at = now
        small = im.reduce(max(1, im.width // 64)).convert('L')
        lum = sorted(small.getdata())
        black = lum[len(lum) // 2] < 14 and lum[int(len(lum) * 0.95)] < 40
        if hold:
            if now > hold[0]: self.egg_hold_end('time limit')
            # a new space during the hold is a loading screen (leaving the match), never the end screen: let go now
            elif self.space != hold[2]: self.egg_fade_frame(im, 'load'); self.egg_hold_end('the game loaded a new scene')
            elif black: hold[1] = 0.0
            elif not hold[1]: hold[1] = now
            elif now - hold[1] > 5:
                self.egg_fade_frame(im, 'back'); self.egg_hold_end('the screen came back without an end screen')
            return
        before = getattr(self, 'egg_last_lit', None)
        if not black: self.egg_last_lit = im; self.egg_black = 0; return
        if not getattr(self, 'egg_black', 0): self.egg_black = now                  # when this black stretch began
        if now - self.egg_black < EGG_BLACK or not C.automation(self.args.cfg, 'egg_fade_hold') or not self.sanctum_in_run(now): return
        # sanctum_in_run remembers a run for 30 s (for reading the end screen); the fade itself has to come while the
        # run is still being seen (SH reads ~1/s and drops it at the fade), and not just after a scene load's fade-in
        if now - getattr(self, 'sh_run_seen', 0) > 4 or now - getattr(self, 'egg_space_at', 0) < 8: return
        if self.hwnd and not wc.is_foreground(self.hwnd): return          # only while you're in the game
        # its own switch: keyboard and mouse are held whatever the result-screen block says, for as long as that allows
        seconds = max(HOLD_MIN, min(120, int(r.get("hold_seconds") or 60)))
        self.egg_hold = [now + seconds, 0.0, self.space]
        self.hold.start(seconds)
        if r.get('hold_controller') and CT.available(): CT.hide_controllers(True)
        log.info(f'Egg Heist: the screen faded to black - holding inputs until the end screen is read (at most {seconds} s)')
        if before is not None: self.egg_fade_frame(before, 'before')

    def egg_fade_frame(self, im, what):
        """Diagnostics (Captures & logs › diagnostic captures): the frame before a fade and the one that ended a hold
        without an end screen go to logs/egg_fade/, to tune what counts as the fade. Small JPEGs, cleared with the rest."""
        if not self.args.cfg['capture'].get('debug_frames'): return
        try:
            d = os.path.join(HERE, 'logs', 'egg_fade'); os.makedirs(d, exist_ok=True)
            small = im.convert('RGB'); small.thumbnail((1280, 720))
            small.save(os.path.join(d, time.strftime('%Y%m%d_%H%M%S_') + what + '.jpg'), quality=80)
        except Exception as e:
            log.info(f'egg fade frame not saved: {e!r}')

    def egg_hold_end(self, why):
        if not getattr(self, 'egg_hold', None): return
        self.egg_hold = None; self.hold.stop()
        if CT.hidden: CT.hide_controllers(False)
        log.info(f'Egg Heist input hold ended: {why}')

    def bar_sample(self, now):
        """Diagnostics: one full-size frame per fight with the boss bar on it (logs/bar_samples/), the material for
        checking the bar at every monitor resolution (tests/bar_resolutions.py). Lossless, at the screen's own size;
        cleared like every other capture (Keep screenshots)."""
        f = self.fight
        if not f or getattr(f, 'sampled', False) or now - f.created < 8 or not self.args.cfg['capture'].get('debug_frames'):
            return
        if f.brk.phase in ('broken', 'refill'): return     # BREAK's gold overlay covers the bar's end: not a clean sample
        f.sampled = True
        try:
            im = self.grab()
            if im is None: return
            d = os.path.join(HERE, 'logs', 'bar_samples'); os.makedirs(d, exist_ok=True)
            im.save(os.path.join(d, os.path.splitext(os.path.basename(f.log.name))[0] + '.png'))
        except Exception as e:
            log.info(f'bar sample not saved: {e!r}')

    def sim_result(self, im, now, u, ux, H):
        """Holo-Battle Sim's end screen: "Victory" with "Battle time 00:05:25 Earth Training - Hard" under it (Season
        1: "Training of Water - Hard"). A win goes to the leaderboards with the screen as proof, and with the setup:
        the team (names and levels, the MVP's shares), the Holo-gizmo counts and coins from this screen, plus the
        Training Style and skills noted before the run (sim_prep_for)."""
        if now - getattr(self, 'sim_seen', 0) < 30: return                 # once per appearance
        box = (ux + u * 0.36, H * 0.198, ux + u * 0.65, H * 0.233)
        t = self.ocr.read({'w': R.prep_white(im, box, u / 2000), 'b': R.prep_bright(im, box, u / 2000),
                           'title': R.prep_bright(im, (ux + u * 0.43, H * 0.13, ux + u * 0.57, H * 0.20), u / 2000),
                           # (the gold title over the faint "SUCCESS" behind it: only its brightest part reads)
                           'title2': R.prep_text(im, (ux + u * 0.43, H * 0.13, ux + u * 0.57, H * 0.20), u / 2000, thr=200),
                           **sim_team_crops(im, u, ux, H)})
        sim = parse_sim(t.get('w')) or parse_sim(t.get('b'))
        # the pale gold title over a light background often doesn't read ('Victor', 'Vi<ory', nothing): a win is a
        # title that looks like Victory, or no 'Defeat' read and the team's cards reading as they do on a Victory
        title = f"{t.get('title') or ''} {t.get('title2') or ''}".lower()
        names = sorted({n for n, _ in self.icons.heads.values()}) if self.icons else ()
        team = parse_sim_team(t, names)
        won = bool(re.search(r'victo|v[il1]\W?[c<(]?t?ory', title)) or ('defe' not in title and len(team['team']) >= 3)
        if not sim:
            log.info(f"Holo-Battle Sim end screen not read: {t!r}"); return
        # The screen slides in: title and team first, the shares, coins and gizmo row a moment later (1 Nov: read
        # 0.3 s in, those were blank and the Gizmo screen never opened). Give them up to 3 s.
        if won and not (t.get('share') or t.get('coins') or t.get('coinsL')):
            first = getattr(self, 'sim_first', None)
            if first is None or now - first > 30: self.sim_first = first = now
            if now - first < 3: return
        self.sim_seen = now
        log.info(f"Holo-Battle Sim: {'victory' if won else 'not a victory'} ({t.get('title')!r}), {sim['trial']} - "
                 f"{sim['difficulty']}, battle time {sim['time_s']} s")
        if not won: self.sim_release('not a victory'); return
        # The same win still up (the game left on the Victory screen, tabbed out): it was sent once already. The
        # 30 s above only spaces the reads; this run went to the leaderboards four times before (1 Nov).
        key = (self.space, sim['trial'], sim['difficulty'], sim['time_s'])
        if key == getattr(self, 'sim_last', None): return
        self.sim_last = key
        low = team.pop('low', False)
        pictures = self.sim_pictures(im, u, ux, H, SIM_LOW if low else 0.0)
        if 'gizmos' not in pictures:
            other = self.sim_pictures(im, u, ux, H, 0.0 if low else SIM_LOW)
            if 'gizmos' in other: pictures, low = other, not low
        self.sim_low = low                                        # the tour clicks the gizmo row at that height
        sim['setup'] = {**team, **pictures, **self.sim_prep_for(sim)}
        counts = sim['setup'].pop('gizmoCounts')
        if sim['setup'].get('gizmos') and len(sim['setup']['gizmos']) == len(counts):
            sim['setup']['gizmos'] = [{**g, 'count': n} for g, n in zip(sim['setup']['gizmos'], counts)]
        elif sim['setup'].get('gizmos'):
            # the series known from their icons, a count under one not read (1 Nov: 6 icons, 5 counts): the series are
            # kept, so the Gizmo screen is still read (it lists each one), with the counts unknown rather than shifted
            log.info(f"Holo-Battle Sim: {len(sim['setup']['gizmos'])} gizmo series but {len(counts)} counts read {counts}: counts left out")
            sim['setup']['gizmos'] = [{**g, 'count': None} for g in sim['setup']['gizmos']]
        else: sim['setup']['gizmos'] = [{'count': n} for n in counts] or None
        log.info(f"Holo-Battle Sim setup: {sim['setup']}")
        shot = self.picture(im, 'screens', 'sim_' + time.strftime('%Y%m%d_%H%M%S'), proof=True)
        self.sim_pending = (sim, shot)
        if not self.sim_tour_start(now): self.sim_send()           # (gives control back too, if the gate took it)

    def sim_send(self, note=None):
        """Note and send the Holo-Battle Sim win waiting in sim_pending (after the Gizmo tour, if one ran), with the
        Gizmo screen's pictures as more proof; the notice on the Victory screen says it's saved."""
        pending, self.sim_pending = getattr(self, 'sim_pending', None), None
        if not pending: return
        sim, shot = pending[:2]
        shots = [os.path.relpath(p, HERE) for p in (sim.pop('gizmo_shots', None) or []) if p]
        for g in sim['setup'].get('gizmos') or []: g.pop('sid', None)
        os.makedirs(os.path.join(HERE, 'logs'), exist_ok=True)
        path = os.path.join(HERE, 'logs', 'sim_runs.csv'); new = not os.path.exists(path)
        with open(path, 'a', newline='', encoding='utf-8') as fh:
            w = csv.writer(fh)
            if new: w.writerow(['time', 'uid', 'trial', 'difficulty', 'time_s', 'screenshot'])
            w.writerow([time.strftime('%Y-%m-%d %H:%M:%S'), self.uid, sim['trial'], sim['difficulty'], sim['time_s'],
                        os.path.relpath(shot, HERE) if shot else ''])
        if self.submitter and self.uid:
            self.submitter.enqueue(S.sim_payload(sim, self.uid), os.path.relpath(shot, HERE) if shot else None, gizmos=shots)
        self.screen_settled('sim_result', time.time())
        self.out('status', f"Holo-Battle Sim recorded: {sim['trial']} - {sim['difficulty']} in {sim['time_s']} s"
                 + (f" ({note})" if note else ''))
        self.sim_release('the run is saved')

    # The Gizmo screen (UI_Pb_Tower_BuffDetail), opened by clicking the Victory screen's Holo-gizmo row: the series
    # down the left ("All 43", then Energy 9 ... Wealth 9), the picked gizmos of the chosen one on the right, each a
    # name in dark text with its rarity, about six to a view (a longer list scrolls). It opens on the first series.
    # "All" lists every row; the game data says each one's series, and each series' rows are checked against the
    # Victory screen's count for it (an upgraded Core, "...: Final", counts 3: sim_count). Measured at 1920x1080.
    GIZMO_TITLE = (0.06, 0.03, 0.14, 0.085)
    GIZMO_ALL = (0.09, 0.16)                  # "All 43" at the top of the series
    GIZMO_LIST = (0.305, 0.09, 0.60, 0.96)    # the names' column (descriptions run on to the right)
    GIZMO_BACK = (0.05, 0.056)
    SIM_PROOF_MAX = 20                        # views of the Gizmo screen sent with a run, at most (~250 KB each)

    def sim_control(self, now, why):
        """aLogs takes control for the Holo-Battle Sim's end (Leaderboards › Automation): your keyboard and mouse
        buttons held (and the controller hidden) as set, for at most "Take control for at most" seconds in all, from
        the exit gate (or the Victory screen) to the Gizmo screen read. Kept when it's already on."""
        ctl = getattr(self, 'sim_ctl', None)
        if ctl: return ctl
        r = self.args.cfg['results']
        seconds = max(HOLD_MIN, min(120, int(r.get('hold_seconds') or 60)))
        self.sim_ctl = ctl = {'until': now + seconds, 'why': why, 'held': bool(r.get('hold_inputs'))}
        if ctl['held']: self.hold.start(seconds)
        if r.get('hold_controller') and CT.available(): CT.hide_controllers(True)
        log.info(f'Holo-Battle Sim: aLogs has control ({why}), for at most {seconds} s')
        return ctl

    def sim_release(self, why):
        if not getattr(self, 'sim_ctl', None): return
        self.sim_ctl = None; self.hold.stop()
        if CT.hidden: CT.hide_controllers(False)
        log.info(f'Holo-Battle Sim: control given back ({why})')

    def sim_check(self, now):
        """Every frame while aLogs has the Holo-Battle Sim's end: control comes back the moment it isn't going as
        planned, never only at the time limit (1 Nov: held 60 s at a gate whose Victory screen never came, the player
        pressing keys all along). The Gizmo screen's tour keeps its own checks (sim_tour_step)."""
        ctl = getattr(self, 'sim_ctl', None)
        if not ctl or getattr(self, 'sim_tour', None): return
        stage = ctl.get('stage')
        if now > ctl['until']: self.sim_release('time limit')
        elif ctl['held'] and not self.hold.active and now - ctl['until'] < -1:    # Ctrl+Shift/Alt+F12: all of it ends
            self.sim_release('you took control back (Ctrl+F12)')
        elif stage == 'gate' and ctl.get('done') and now - ctl['done'] > SIM_VICTORY_WAIT:
            self.sim_release(f'no Victory screen {SIM_VICTORY_WAIT} s after your press at the gate')

    def sim_gate(self, im, now):
        """Holo-Battle Sim, last level: the exit gate's "End Holo-Battle Sim" button (a dark pill right of the middle
        of the screen, the key on its left). aLogs presses nothing here: with the button up it watches for your own
        press of it (the controller's Y or the keyboard's F, CT.PressWatch), and from that press it takes control, so
        the Victory screen can't be skipped (sim_result takes over there; control comes back if it doesn't come,
        sim_check). 1 Nov: aLogs pressing F itself didn't work with a controller in use (the game ignores the keyboard
        while a controller is held, and a controller already open can't be blocked), so your press ends the run. Only
        on the last level ("Level 16/16" top left), so an early press isn't taken for the end, and only for the button,
        not the gate's own floating label (same words, higher up)."""
        if (not self.holosim_space or self.args.replay is not None or not self.hwnd or not wc.is_foreground(self.hwnd)
                or not C.automation(self.args.cfg, 'sim_gate') or getattr(self, 'sim_tour', None)): return
        gate = getattr(self, 'sim_gate_state', None) or {}
        if gate.get('space') != self.space: gate = {'space': self.space}          # a new run: its level read again
        watch = getattr(self, 'sim_press', None) or CT.PressWatch(); self.sim_press = watch
        if gate.get('last'): watch.start()                         # all through the last level: a quick tap counts too
        press = watch.last
        # a press while the button is up, or just before it was seen (it shows a moment before a look finds it; 1 Nov:
        # Y pressed the instant the button came up was missed), and not one already taken
        if (press and gate.get('first_up') and press[1] >= gate['first_up'] - SIM_PRESS_EARLY
                and press[1] > gate.get('used', 0) and now - gate.get('up_at', 0) <= SIM_GATE_GONE
                and not getattr(self, 'sim_ctl', None)):
            what, at = press; gate['used'] = at; watch.stop()
            ctl = self.sim_control(now, 'your press at the exit gate')     # which key isn't logged
            ctl.update(stage='gate', done=at); gate['first_up'] = gate['up_at'] = None
            log.info('Holo-Battle Sim: waiting for the Victory screen')
            return
        if now - gate.get('look', 0) < 0.15: return       # the level and the button in one look: no wait on either
        gate['look'] = now; self.sim_gate_state = gate
        W, H = im.size; u = min(W, H * 16 / 9); ux = (W - u) / 2; k = u / 2000
        if not gate.get('last'):                                   # the level, read until it's the last one
            t = self.ocr.read({'l': R.prep_bright(im, (ux + u * 0.02, H * 0.015, ux + u * 0.20, H * 0.08), k)})
            m = re.search(r'(\d{1,2})\s*/\s*(\d{1,2})', t.get('l') or '')
            gate['last'] = bool(m and m.group(1) == m.group(2) and int(m.group(1)) > 1)
            if not gate['last']: return
            log.info(f"Holo-Battle Sim: last level ({t.get('l')!r}): the exit gate is watched")
        if getattr(self, 'sim_ctl', None): return
        # the button: its pill's colour first (~10 ms at 4K), its words only when the pill is there (~50 ms to prepare
        # and read). Both read on every level, it was ~60 ms a look, 0.15 s apart; the words alone are the gate's own
        # label too
        box = (ux + u * 0.50, H * 0.40, ux + u * 0.90, H * 0.66)
        up = sim_gate_pill(im, box, u) and 'endholo' in re.sub(r'[^a-z]', '', (self.ocr.read({'b': R.prep_bright(im, box, k)}).get('b') or '').lower())
        if up:
            if not gate.get('first_up'):
                gate['first_up'] = now; log.info('Holo-Battle Sim: the exit gate button is up; watching for your Y or F')
            gate['up_at'] = now
        elif gate.get('first_up') and now - gate['up_at'] > SIM_GATE_GONE:
            gate['first_up'] = gate['up_at'] = None               # walked away from the gate: no press counts now

    def sim_tour_start(self, now):
        """After a Holo-Battle Sim win, open the Gizmo screen and read every gizmo picked (All), keeping pictures of
        the list as proof: only with the automation on (Leaderboards › Automation › read the gizmos), the
        game the active window and the game's gizmo names known. False when it doesn't start."""
        if (not C.automation(self.args.cfg, 'sim_gizmos') or self.args.replay is not None or not self.hwnd
                or not wc.is_foreground(self.hwnd) or not self.icons or not self.icons.sim_gizmos):
            return False
        gizmos = self.sim_pending[0]['setup'].get('gizmos') or []
        if not gizmos or not all(g.get('sid') for g in gizmos): return False
        self.sim_control(now, 'the Victory screen')
        self.sim_tour = {'stage': 'open', 'next': now + 0.3, 'tries': 0, 'rows': [], 'read': {}, 'idle': 0, 'scrolls': 0,
                         'shots': [], 'want': sum(g['count'] for g in gizmos) if all(g.get('count') is not None for g in gizmos) else None}
        log.info(f"Holo-Battle Sim: opening the Gizmo screen for {self.sim_tour['want'] if self.sim_tour['want'] is not None else 'all its'} gizmos")
        return True

    def sim_tour_end(self, why):
        tour, self.sim_tour = getattr(self, 'sim_tour', None), None
        if not tour: return
        self.sim_seen = time.time()                            # back on the Victory screen: not a new win
        pending = getattr(self, 'sim_pending', None)
        got = 0
        if pending:
            setup = pending[0]['setup']
            for g in setup.get('gizmos') or []:
                rows = tour['read'].get(g['sid'])
                if rows is not None:
                    g['picked'] = [{'name': n, 'rarity': self.sim_rarity(n)} for n in rows]
                    g['complete'] = self.sim_count(rows) == g.get('count'); got += self.sim_count(rows)
            cores = [p['name'] for g in setup.get('gizmos') or [] for p in g.get('picked') or [] if p['rarity'] == 'Core']
            if cores: setup['cores'] = cores
            pending[0]['gizmo_shots'] = tour['shots']
        of = '' if tour['want'] is None else f" of {tour['want']}"
        log.info(f"Holo-Battle Sim Gizmo screen: {why}; read {got}{of} gizmos: "
                 + '; '.join(f"{sid}: {rows}" for sid, rows in tour['read'].items()))
        self.sim_send(note=f"{got} of {tour['want']} gizmos" if tour['want'] is not None and got != tour['want'] else f"{got} gizmos")
        self.sim_release('the Gizmo screen is read')

    def sim_rarity(self, name):
        """A gizmo's rarity from the game data; an upgraded Core shows as "Predictive Dice: Final" (data: Predictive Dice)."""
        g = self.icons.sim_gizmos.get(name) or self.icons.sim_gizmos.get(name.split(':')[0].strip())
        return g[1] if g else None

    def sim_names(self, text, sid=None):
        """The gizmo names in one view of the list, top to bottom, matched to the names in the game data (one series')
        (descriptions and rarity tags are skipped). A name with ": Final" (an upgraded Core) keeps it."""
        norm = lambda x: re.sub(r'[^a-z]', '', x.lower())
        names = [n for n, (s, _) in self.icons.sim_gizmos.items() if sid is None or s == sid]
        out = []
        for line in text.split(' / '):
            line = re.sub(r'\b(Normal|Uncommon|Core)\b', '', line).strip()
            final = norm(line).endswith('final')                   # "Predictive Dice: Final" (the colon isn't always read)
            head = re.sub(r':?\s*final\s*$', '', line, flags=re.I) if final else line.partition(':')[0]
            if len(norm(head)) < 4 or not names: continue
            score, name = max((difflib.SequenceMatcher(None, norm(head), norm(n)).ratio(), n) for n in names)
            if score < 0.85: continue
            out.append(name + ': Final' if final and not norm(name).endswith('final') else name)
        return out

    @staticmethod
    def sim_stitch(rows, view):
        """Add one view's names to the list read so far: the longest end of `rows` the view starts with is the overlap
        (a scroll keeps a few rows in view), the rest is new. A gizmo picked twice stays twice."""
        base = lambda names: [n.split(':')[0] for n in names]      # a Final read without its suffix is still the same row
        for k in range(min(len(rows), len(view)), 0, -1):
            if base(rows[-k:]) == base(view[:k]): return rows + view[k:]
        return rows + view

    @staticmethod
    def sim_count(rows):
        """The picks a series' rows stand for: an upgraded Core ("...: Final") counts 3, the gizmo and its two upgrades
        (1 Nov: 41 rows for the 43 the Victory screen counts, one of them Predictive Dice: Final)."""
        return sum(3 if n.endswith(': Final') else 1 for n in rows)

    def sim_tour_step(self, im, now):
        """One step of the Gizmo tour, on each new frame while it runs. True while the tour has the screen.
        open the Gizmo screen -> click All -> read the names in view, scroll and read again until every pick is read or
        the list stops changing (a picture of each view kept) -> back to the Victory screen."""
        tour = getattr(self, 'sim_tour', None)
        if not tour: return False
        ctl = getattr(self, 'sim_ctl', None)
        if not ctl or now > ctl['until']: self.sim_tour_end('time limit'); return False
        if ctl['held'] and not self.hold.active:                  # Ctrl+Shift/Alt+F12: the tour stops clicking too
            self.sim_tour_end('you took control back (Ctrl+F12)'); return False
        if not wc.is_foreground(self.hwnd): self.sim_tour_end('game is not the active window'); return False
        if now < tour['next'] or im is None: return True
        W, H = im.size; u = min(W, H * 16 / 9); ux = (W - u) / 2; k = u / 2000
        at = lambda x, y: (self.origin[0] + ux + u * x, self.origin[1] + H * y)
        stage = tour['stage']
        if stage == 'open':                                     # the first icon of the Victory screen's gizmo row
            tour['tries'] += 1
            if tour['tries'] > 3: self.sim_tour_end(f"the Gizmo screen did not open (title read {tour.get('title', '')!r})"); return False
            spot = at(SIM_GIZMO_X, 0.545 + (SIM_LOW if getattr(self, 'sim_low', False) else 0.0))
            log.info(f"Holo-Battle Sim: clicking the gizmo row at {tuple(round(v) for v in spot)} (try {tour['tries']})")
            wc.click_at(*spot); tour.update(stage='wait', next=now + 0.3, waited=0)
            return True
        title = self.ocr.read({'t': R.prep_grey(im, (ux + u * self.GIZMO_TITLE[0], H * self.GIZMO_TITLE[1],
                                                     ux + u * self.GIZMO_TITLE[2], H * self.GIZMO_TITLE[3]), k)}).get('t') or ''
        on_page = 'gizmo' in re.sub(r'[^a-z]', '', title.lower())
        if stage == 'wait':
            tour['title'] = title
            # looked at about every 0.25 s; clicked again only when the screen hasn't come after 1.5 s
            if on_page: tour.update(stage='all', next=now)
            else:
                tour['waited'] += 1
                tour.update(stage='open' if tour['waited'] >= 6 else 'wait', next=now + 0.25)
            return True
        if stage == 'back':           # the back arrow didn't always take (the tour was left on the Gizmo screen): Esc
            if not on_page: self.sim_tour_end('done'); return True
            tour['backs'] += 1
            if tour['backs'] > 4: self.sim_tour_end('the Gizmo screen did not close'); return False
            if tour['backs'] % 2: wc.press_key(wc.VK_ESCAPE)
            else: wc.click_at(*at(*self.GIZMO_BACK))
            tour['next'] = now + 0.4
            return True
        if not on_page: self.sim_tour_end('left the Gizmo screen'); return False
        if stage == 'all':
            wc.click_at(*at(*self.GIZMO_ALL)); tour.update(stage='read', next=now + 0.35)
            return True
        # 'read': once the list stands still (its text the same two looks running, at most 1.5 s), the names in view;
        # scroll on about 3 rows a time (each view shares rows with the last) until every pick is read or a scroll
        # doesn't move the list (its end). Each view with new rows is kept as proof, as it was on screen.
        x0, y0, x1, y1 = self.GIZMO_LIST
        sig = gizmo_list_sig(im)
        if not gizmo_list_same(sig, tour.get('sig')) and now - tour.get('moved', now) < 1.5:
            if 'moved' not in tour: tour['moved'] = now
            tour['sig'] = sig; tour['next'] = now + 0.08
            return True
        tour.pop('moved', None); tour['sig'] = sig
        at_end = gizmo_list_same(sig, tour.get('read_sig')) and tour['scrolls'] > 0
        tour['read_sig'] = sig
        text = self.ocr.read({'n': R.prep_grey(im, (ux + u * x0, H * y0, ux + u * x1, H * y1), k, size=2)}).get('n') or ''
        view = self.sim_names(text)
        rows = self.sim_stitch(tour['rows'], view)
        if tour['rows'] and view and len(rows) == len(tour['rows']) + len(view) and tour.get('backups', 0) < 3:
            # no row in common with the last view: the scroll went too far, so back up a little and read again
            tour['backups'] = tour.get('backups', 0) + 1; tour['next'] = now + 0.12
            wc.scroll_at(*at((x0 + x1) / 2, (y0 + y1) / 2), 2)
            return True
        if len(rows) > len(tour['rows']):
            shot = self.sim_tour_shot(im, f"all_{tour['scrolls']:02d}")
            if shot: tour['shots'].append(shot)
        tour['idle'] = 0 if len(rows) > len(tour['rows']) else tour['idle'] + 1
        tour['rows'] = rows
        if (tour['want'] is None or self.sim_count(rows) < tour['want']) and not at_end and tour['idle'] < 2 and tour['scrolls'] < 20:
            tour['scrolls'] += 1; tour['next'] = now + 0.12
            wc.scroll_at(*at((x0 + x1) / 2, (y0 + y1) / 2), -3)
            return True
        for n in rows:                                          # each row under its series (game data)
            g = self.icons.sim_gizmos.get(n.split(':')[0].strip())
            if g: tour['read'].setdefault(g[0], []).append(n)
        wc.click_at(*at(*self.GIZMO_BACK)); tour.update(stage='back', backs=0, next=now + 0.4)
        return True

    def sim_tour_shot(self, im, what):
        """A view of the Gizmo list for the run's proof (logs/screens, sent with the run, then deleted like the others):
        the whole screen as it stood, nothing cut, joined or drawn on; the site shows the views one under the other."""
        tour = self.sim_tour
        if len(tour['shots']) >= self.SIM_PROOF_MAX: return None
        return self.picture(im, 'screens', f"sim_gizmo_{time.strftime('%Y%m%d_%H%M%S')}_{len(tour['shots']):02d}_{re.sub(r'[^A-Za-z0-9]', '', what)}",
                            proof=True, fresh=False)

    def sim_pictures(self, im, u, ux, H, dy=0.0):
        """The Victory screen's pictures, matched to the game's own (icons.py): the Training Style, from the Core
        gizmo the run started with (Valor's Fervor Mask); the Alpha Transformation (Scorchhowl); and which series
        each icon of the Holo-gizmo row is, left to right (the game lists them in series order, skipping empty ones).
        Only clear matches are kept; {} without the game's icons."""
        ic = self.icons
        if not ic or not ic.sim_art:
            log.info(f"Holo-Battle Sim pictures: not read (the game's icons {self.icons_state or 'not loaded'})"); return {}
        b = lambda cx, cy, r: (ux + u * (cx - r), H * (cy + dy) - u * r, ux + u * (cx + r), H * (cy + dy) + u * r)
        out = {}
        core = ic.sim_picture(im, b(0.571, 0.41, 0.04), 'core')
        if core and core[1] >= 0.3 and core[2] >= 0.1:
            out['style'] = ic.sim[core[0]].get('name'); out['core'] = ic.sim[core[0]].get('core')
        tf = ic.sim_picture(im, b(0.472, 0.41, 0.032), 'transform')
        if tf and tf[1] >= 0.35 and tf[2] >= 0.1: out['transform'] = ic.sim[tf[0]].get('transform')
        row = []
        for i in range(SIM_GIZMOS):
            cx = SIM_GIZMO_X + SIM_GIZMO_PITCH * i
            g = ic.sim_glyph(im, (ux + u * (cx - 0.02), H * (0.512 + dy), ux + u * (cx + 0.02), H * (0.578 + dy)))
            if g is None: break
            if g[1] < 0.45 or g[2] < 0.05 or (row and int(g[0]) <= int(row[-1])): row = None; break   # unsure / out of order
            row.append(g[0])
        if row: out['gizmos'] = [{'series': ic.sim[s].get('name'), 'sid': s} for s in row]
        log.info(f'Holo-Battle Sim pictures{" (lower)" if dy else ""}: core {core}, transform {tf}, series row {row}')
        return out

    def sim_prep_for(self, sim):
        """Each Aniimo's skills noted before this run (the Party screen), if seen in the last 2 hours. {} when not.
        (The Training Style comes from the Victory screen's Core gizmo.)"""
        prep = getattr(self, 'sim_prep', None)
        if not prep or time.time() - prep.get('at', 0) > 7200: return {}
        return {k: v for k, v in prep.items() if k == 'skills'}

    def egg_result(self, im, now, u, ux, H, title):
        """Egg Heist end screen ("Adventure Successful", or the failed one): difficulty, mode, time, objective, Loot
        This Match, Total Defeats and today's chests. One record per match in logs/egg_matches.csv, with the
        screen kept as proof."""
        if now - getattr(self, 'egg_seen', 0) < 3: return
        self.egg_seen = now
        k = u / 2000
        box = lambda a, b, c, d: (ux + u * a, H * b, ux + u * c, H * d)
        t = self.ocr.read({
            'difficulty': R.prep_bright(im, box(0.035, 0.2, 0.088, 0.232), k),
            'mode': R.prep_text(im, box(0.09, 0.2, 0.235, 0.232), k),
            'time': R.prep_short(im, box(0.262, 0.2, 0.31, 0.235), k),
            'goal': R.prep_text(im, box(0.05, 0.258, 0.26, 0.292), k),
            'chests': R.prep_bright(im, box(0.18, 0.81, 0.305, 0.84), k)})
        # Loot This Match and Total Defeats sit 4% higher when there's no objective line above them (Team Mode has
        # none): read them where this screen's layout puts them, and the other way if that gives no number.
        def numbers(dy):
            return self.ocr.read({
                'loot': R.prep_text(im, box(0.03, 0.345 + dy, 0.15, 0.42 + dy), k),
                'defeats': R.prep_labelled(im, box(0.03, 0.465 + dy, 0.075, 0.535 + dy), k, 'Defeats:'),
                'defeats2': R.prep_labelled(im, box(0.03, 0.465 + dy, 0.075, 0.535 + dy), k, 'Defeats:', size=3),
                'defeats3': R.prep_labelled(im, box(0.03, 0.465 + dy, 0.075, 0.535 + dy), k, 'Defeats:', size=5)})   # 720p
        shifts = (0.0, -0.04) if (t.get('goal') or '').strip() else (-0.04, 0.0)
        t.update(numbers(shifts[0]))
        if first_num((t.get('loot') or '').replace(',', '').replace(' ', '')) is None: t.update(numbers(shifts[1]))
        low = title.lower()
        outcome = 'success' if 'success' in low else 'failed' if ('fail' in low or 'unsuccess' in low) else title.strip()
        m = re.search(r'(\d{1,2})\s*:\s*(\d{2})', t.get('time', ''))
        seconds = int(m.group(1)) * 60 + int(m.group(2)) if m else None
        loot = first_num((t.get('loot') or '').replace(',', '').replace(' ', ''))
        # the number after the drawn label; a lone 1 often reads as I, l or |, a 0 as O
        def digits(text):
            raw = (text or '').split(':', 1)[-1].strip().translate(str.maketrans('Il|!Oo', '111100')).replace(' ', '')
            return int(raw) if re.fullmatch(r'\d+', raw) else None
        defeats = digits(t.get('defeats'))
        if defeats is None: defeats = digits(t.get('defeats2'))
        if defeats is None: defeats = digits(t.get('defeats3'))
        cm = re.search(r'(\d+)\s*/\s*(\d+)', t.get('chests', ''))
        difficulty = next((d for d in ('Chaos', 'Nightmare', 'Expert', 'Hard', 'Normal', 'Easy') if d.lower() in t.get('difficulty', '').lower()),
                          (t.get('difficulty') or '').strip() or None)
        mode = (t.get('mode') or '').split(':', 1)[-1].strip()
        match = {'outcome': outcome, 'difficulty': difficulty, 'mode': canon(mode, EGG_MODES) if mode else None,
                 'time_s': seconds, 'objective': (t.get('goal') or '').strip() or None, 'loot': loot, 'defeats': defeats,
                 'chests_today': f'{cm.group(1)}/{cm.group(2)}' if cm else None}
        log.info(f'Egg Heist result: OCR {t!r} -> {match}')
        if loot is None and seconds is None: return                  # still sliding in: the next frame reads it
        self.egg_hold_end('end screen read')
        key = (outcome, loot, seconds, defeats)
        if self.last_scores.get('egg_result') == key: return
        self.last_scores['egg_result'] = key
        shot = self.picture(im, 'screens', 'egg_result_' + time.strftime('%Y%m%d_%H%M%S'), proof=True)
        path = os.path.join(HERE, 'logs', 'egg_matches.csv'); new = not os.path.exists(path)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, 'a', newline='', encoding='utf-8') as f:
            w = csv.writer(f)
            if new: w.writerow(['time', 'uid', 'outcome', 'difficulty', 'mode', 'time_s', 'objective', 'loot', 'defeats', 'chests_today', 'screenshot'])
            w.writerow([time.strftime('%Y-%m-%d %H:%M:%S'), self.uid, outcome, difficulty, match['mode'], seconds, match['objective'],
                        loot, defeats, match['chests_today'], os.path.relpath(shot, HERE) if shot else ''])
        # to the leaderboards (Egg Heist > Matches), with the end screen as proof
        if self.submitter and self.uid:
            self.submitter.enqueue(S.egg_match_payload(match, getattr(self, 'egg_season', None), self.uid),
                                   os.path.relpath(shot, HERE) if shot else None)
        self.screen_settled('egg_result', time.time())
        self.out('status', f"Egg Heist match recorded: {'escaped' if outcome == 'success' else outcome}"
                 + (f', {loot:,} loot' if loot is not None else '') + (f' ({difficulty})' if difficulty else ''))

    def egg_rank_info(self, im, now, u, ux, H):
        """Egg Heist "Rank Info": rank title, "Rank Points X/Y" (below Egg Emperor) or the star count (at Egg
        Emperor), and the season badge. Recorded as seen; the sub-tier (IV, III, ...) comes from the points."""
        if now - self.stats_seen < 5: return
        self.stats_seen = now
        k = u / 2000
        t = self.ocr.read({
            'title': R.prep_text(im, (ux + u * 0.10, H * 0.195, ux + u * 0.365, H * 0.275), k),
            'points': R.prep_text(im, (ux + u * 0.105, H * 0.80, ux + u * 0.36, H * 0.86), k),
            # the star count (Egg Emperor) under the badge: Windows OCR is picky about the margins around it, so three
            # boxes are read and the first one that gives a number counts (checked at 1080p and 4K)
            'stars': R.prep_short(im, (ux + u * 0.215, H * 0.715, ux + u * 0.29, H * 0.785), k),
            'stars2': R.prep_short(im, (ux + u * 0.215, H * 0.715, ux + u * 0.30, H * 0.785), k),
            'stars3': R.prep_short(im, (ux + u * 0.225, H * 0.72, ux + u * 0.28, H * 0.78), k),
            'badge': R.prep_short(im, (ux + u * 0.918, H * 0.06, ux + u * 0.962, H * 0.125), k),
            'season': R.prep_text(im, (ux + u * 0.67, H * 0.05, ux + u * 0.91, H * 0.13), k)})
        points, next_start = E.parse_points(t.get('points'))
        stars = next((n for n in (first_num(t.get(key)) for key in ('stars', 'stars2', 'stars3')) if n is not None), None)             if points is None else None
        st = E.standing(t.get('title'), points, stars)
        # the season title can miss a frame (it fades in): then today's season by date, never an entry without one
        season = E.parse_season(t.get('badge'), t.get('season')) or getattr(self, 'egg_season', None) or E.current_season()
        if season: self.egg_season = season
        log.info(f'Egg Heist Rank Info: OCR {t!r} -> {st}, season {season}, next big rank at {next_start}')
        if not st or (st['points'] is None and st['stars'] is None): return
        key = (season, st['name'], st['points'], st['stars'])
        if self.last_scores.get('egg_rank') == key: return
        self.last_scores['egg_rank'] = key
        d = os.path.join(HERE, 'logs', 'screens'); os.makedirs(d, exist_ok=True)
        shot = self.picture(im, 'screens', 'egg_rank_' + time.strftime('%Y%m%d_%H%M%S'), proof=True)
        path = os.path.join(HERE, 'logs', 'scores.csv'); new = not os.path.exists(path)
        with open(path, 'a', newline='', encoding='utf-8') as f:
            w = csv.writer(f)
            if new: w.writerow(['time', 'uid', 'cycle', 'screen', 'boss', 'score', 'stars', 'screenshot'])
            w.writerow([time.strftime('%Y-%m-%d %H:%M:%S'), self.uid, season, 'egg_rank', st['name'],
                        st['points'], st['stars'], os.path.relpath(shot, HERE) if shot else ''])
        self.screen_settled('egg_rank', time.time())
        self.out('status', f"Egg Heist rank recorded: {st['name']}"
                 + (f" - {st['points']:,} rank points" if st['points'] is not None else '')
                 + (f" - {st['stars']:,} stars" if st['stars'] is not None else ''))
        if self.submitter:
            self.submitter.enqueue(S.eggheist_payload(st, season, self.uid, self.egg_profile(season, nextRankAt=next_start)),
                                   os.path.relpath(shot, HERE) if shot else None)

    def egg_profile(self, season, **values):
        """The season's numbers for the player's profile, kept in logs/egg_profile.json so the rank screens add to them
        (Rank Info's threshold, the post-match screen's gain and bar) instead of each sending only its own."""
        path = os.path.join(HERE, 'logs', 'egg_profile.json')
        try:
            with open(path, encoding='utf-8') as f: kept = json.load(f)
        except (OSError, ValueError): kept = {}
        mine = kept.setdefault(f'{self.uid}|{season}', {})
        mine.update({k: v for k, v in values.items() if v is not None})
        try:
            S.write_json(path, kept)
        except OSError: pass
        return dict(mine)

    def egg_rank_gain(self, im, now, u, ux, H, points_text):
        """Egg Heist's rank screen after a match: the rank title over the badge, the Rank Points bar "1337/1350" (the
        total and where the next egg fills) and what the match added ("+18"). The total updates the season's ladder
        entry; the bar and the gain go to the player's profile. The match's grade (B) is left out on purpose."""
        if now - getattr(self, 'egg_gain_seen', 0) < 5: return
        self.egg_gain_seen = now
        k = u / 2000
        box = lambda a, b, c, d: (ux + u * a, H * b, ux + u * c, H * d)
        t = self.ocr.read({'title': R.prep_bright(im, box(0.38, 0.215, 0.62, 0.265), k),
                           'title2': R.prep_text(im, box(0.37, 0.20, 0.63, 0.275), k, thr=230),
                           'gain': R.prep_text(im, box(0.85, 0.555, 0.90, 0.605), k)})
        points, bar_end = E.parse_points(points_text)
        gain = E.parse_gain(t.get('gain'))
        title = t.get('title') if E.tier_of(t.get('title')) else t.get('title2')
        st = E.standing(title, points)
        season = getattr(self, 'egg_season', None) or E.current_season()
        log.info(f'Egg Heist rank after a match: OCR {t!r}, {points_text!r} -> {st}, +{gain}, bar to {bar_end}, season {season}')
        if not st or points is None or gain is None: return
        key = (season, points, gain)
        if self.last_scores.get('egg_gain') == key: return
        self.last_scores['egg_gain'] = key
        self.out('status', f"Egg Heist: +{gain} Rank Points ({st['name']}, {points:,})")
        if self.submitter:
            profile = self.egg_profile(season, lastGain=gain, bar=[points, bar_end], barAt=int(time.time()))
            self.submitter.enqueue(S.eggheist_payload(st, season, self.uid, profile), None)

    def egg_emperor_count(self, im, now, u, ux, H, count_text):
        """The same post-match screen at Egg Emperor, the top rank: no points bar, just the eggs collected ("x57"),
        which keep adding up as you play. They're the season's standing at that rank (the ladder sorts by rank, then
        eggs)."""
        if now - getattr(self, 'egg_gain_seen', 0) < 5: return
        self.egg_gain_seen = now
        k = u / 2000
        box = lambda a, b, c, d: (ux + u * a, H * b, ux + u * c, H * d)
        t = self.ocr.read({'title': R.prep_text(im, box(0.37, 0.20, 0.63, 0.275), k, thr=230),
                           'title2': R.prep_bright(im, box(0.38, 0.215, 0.62, 0.265), k)})
        title = t.get('title') if E.tier_of(t.get('title')) else t.get('title2')
        eggs = first_num(count_text)
        st = E.standing(title, None, eggs) if E.tier_of(title) == E.TOP_TIER else None
        season = getattr(self, 'egg_season', None) or E.current_season()
        log.info(f'Egg Heist rank after a match (Egg Emperor): OCR {t!r}, {count_text!r} -> {st}, season {season}')
        if not st or eggs is None: return
        key = (season, 'emperor', eggs)
        if self.last_scores.get('egg_gain') == key: return
        self.last_scores['egg_gain'] = key
        self.out('status', f"Egg Heist: Egg Emperor, {eggs:,} eggs")
        if self.submitter:
            # at the top rank there's no bar or next rank any more: the eggs (stars) are the whole standing
            profile = {k: v for k, v in self.egg_profile(season).items() if k not in ('bar', 'barAt', 'lastGain', 'nextRankAt')}
            self.submitter.enqueue(S.eggheist_payload(st, season, self.uid, profile), None)

    def challenge_stats(self, im, now, u, ux, H):
        """Esc menu in boss rush: 'Challenge Stats' with this week's Best Score (sum of the best score on each boss)
        and Current Best stars. Recorded to logs/scores.csv (one row per new value) with a screenshot."""
        if now - self.stats_seen < 5: return
        self.stats_seen = now
        k = u / 2000
        text = self.ocr.read({'score': R.prep_text(im, (ux + u * 0.31, H * 0.325, ux + u * 0.455, H * 0.415), k, size=1),
                              'stars': R.prep_short(im, (ux + u * 0.555, H * 0.33, ux + u * 0.62, H * 0.41), k)})
        num = lambda s: int(re.sub(r'\D', '', s.replace('O', '0').replace('o', '0'))) if re.search(r'\d', s or '') else None
        score, stars = num(text.get('score')), first_num(text.get('stars'))
        log.info(f'Challenge Stats screen: OCR {text!r} -> best score {score}, stars {stars}')
        if score is not None: self.record_score(im, 'challenge_stats', 'all (weekly best)', score, stars)

    def prep_screen(self, im, now, u, ux, H, pill):
        """Battle prep screen of one boss: its name top-left, 'Best score: 24459 | (star) 15' top-right."""
        if now - self.stats_seen < 2: return
        self.stats_seen = now; keep_sample(im, 'prep')      # its boss name stopped reading after the Whisperwake update
        t = self.ocr.read({'n': R.prep_text(im, (ux + u * 0.065, H * 0.025, ux + u * 0.3, H * 0.085), u / 2000),
                           'label': R.prep_label(im, u, ux, H)})
        name = t.get('n', '')
        # The big slanted label says which spot the boss has on the boss rush map ('[Middle] Omega Infergon'); the
        # week's game data says which boss is in each spot. That is far more reliable than the small name top-left,
        # which sits over the boss art. The name (top-left, then the label's own) is the fallback.
        label = (t.get('label') or '').lower()
        slot = next((s for s, words in (('left', ('left',)), ('middle', ('middle', 'midd', 'iddle')), ('right', ('right', 'ight')))
                     if any(w in label for w in words)), None)
        slot_id = R.active_cycle_slots().get(slot) if slot else None
        slot_row = next((r for r in self.catalog if r['puppetId'] == slot_id and r['table'] == 'puppet_data' and r['bossTitle_en']), None) if slot_id else None
        nums = re.findall(r'\d[\d,]*', pill.split(':', 1)[-1])
        score = int(nums[0].replace(',', '')) if nums else None
        stars = int(nums[-1].replace(',', '')) if len(nums) > 1 else None
        boss = slot_row['bossTitle_en'] if slot_row else (self.cycle_boss(name.strip(' /|'))
                                                         or self.cycle_boss(label.split('/')[-1].strip()))
        self.read_party(im, now, u, ux, H)
        row = self.stage_first(slot_row or next((r for r in self.catalog if r['table'] == 'puppet_data' and r['bossTitle_en'] == boss
                                                 and int(r['puppetId']) in R.active_cycle_bosses()), None))
        if row: self.prep_boss = (now, row); boss = row['bossTitle_en'] or boss       # the best goes under that boss
        score = interlink_score(score, stars, clear_stars(row['puppetId']) if row else None,
                                two_boss=bool(row and R.stage_of(row['puppetId'])[1]))      # a two-boss stage's best: as read
        log.info(f'battle prep screen: OCR pill {pill!r}, name {name!r} -> {boss}: best score {score}, stars {stars}')
        # the score can read before the stars ('14026', then '14026 | 14' two seconds later sent a best without its
        # stars, 30 Oct): only the same score and stars on two reads in a row count
        key = (boss, score, stars)
        agreed = getattr(self, 'prep_read', None) == key; self.prep_read = key
        if score is not None and stars is not None and boss and agreed: self.record_score(im, 'battle_prep', boss, score, stars)

    def stage_first(self, row):
        """The boss a run on this boss's stage starts with, and keeps its best under: a two-boss middle fight's second
        boss (R.stage_of order 2: the week's bossId_2) gives its stage's first, the bossId. The level graph spawns the
        bossId as the level starts and the bossId_2 only on its death (Scene.1070 scene_graph_data[78824533], nodes
        1-4 and 16-19), and the game keeps one best per stage under the bossId (BossRushMainCtrl.renderBossInfo, line
        350). So a prep or Party screen that names the second boss (how a two-boss stage's screens name it isn't known
        until a live run shows them) neither starts the run on it nor files its best under it. Any other row as is."""
        if not row: return row
        _, order, first = R.stage_of(row['puppetId'])
        if order != 2: return row
        return next((r for r in self.catalog if r['table'] == 'puppet_data' and r['puppetId'] == first
                     and r['bossTitle_en'] and r['fixed_hp_rate']), row)

    def cycle_boss(self, text):
        """The boss name on the battle prep screen, matched to one of this week's three bosses. The boss art behind
        the name makes OCR misread it ('Omega eking'), so a read only counts when it clearly matches one; else ''."""
        import difflib
        ids = {str(b) for b in R.active_cycle_bosses()}
        names = sorted({r['bossTitle_en'] for r in self.catalog if r['puppetId'] in ids and r['table'] == 'puppet_data' and r['bossTitle_en']})
        def norm(s): return re.sub(r'[^a-z]', '', s.lower())
        if not names:
            # a week the game data doesn't list yet: the closest of every boss, only when it's a clear match
            # (OCR adds marks the name doesn't have: 'Omega Tucking$' for the screen's 'Omega Tucking+')
            every = sorted({r['bossTitle_en'] for r in self.catalog if r['table'] == 'puppet_data' and r['bossTitle_en']})
            scored = sorted(((difflib.SequenceMatcher(None, norm(text), norm(n)).ratio(), n) for n in every), reverse=True)
            # (an exact match, marks aside, always wins: the game data also has near-twins like 'Omega Tuckin')
            if scored and (scored[0][0] == 1.0 or scored[0][0] >= 0.85 and (len(scored) < 2 or scored[0][0] - scored[1][0] >= 0.1)):
                return scored[0][1]
            return text
        scored = sorted(((difflib.SequenceMatcher(None, norm(text), norm(n)).ratio(), n) for n in names), reverse=True)
        best, name = scored[0]
        runner_up = scored[1][0] if len(scored) > 1 else 0
        if best >= 0.6 and best - runner_up >= 0.1: return name
        # part of a name ('gon', 'rgon' over the boss art) that fits exactly one of the week's bosses
        part = norm(text)
        fits = [n for n in names if len(part) >= 3 and part in norm(n)]
        if len(fits) == 1: return fits[0]
        log.info(f"battle prep screen: boss name {text!r} clearly matches none of this week's bosses ({scored[:2]})")
        return ''

    def read_party(self, im, now, u, ux, H):
        """The Party row of the battle prep screen: four slots, each a name under a badge - '1P'..'4P' in colour for a
        player (1P yellow, 2P green, 3P light blue, 4P pink), a white 'Ai' for an AI teammate - and your own name in
        green. The result screen and Battle Data say neither, so the run takes both from here (matched by name). The
        row sits higher or lower with the boss's challenge text, so it is found by slot 1's yellow '1P' badge."""
        x0 = int(ux + u * 0.5125); x1 = int(x0 + u * 0.016); top = (0, None)
        for y in range(int(H * 0.58), int(H * 0.72), 2):
            yellow = sum(1 for x in range(x0, x1, 2) for p in [im.getpixel((x, y))[:3]] if p[0] >= 0xd0 and p[1] >= 0xb8 and p[2] <= 0x90)
            if yellow > top[0]: top = (yellow, y)
        if top[0] < 8: return                                         # still sliding in: read it again in a moment
        yc = top[1] / H
        k = u / 2000; crops = {}; kinds = []; mine = []
        for i in range(4):
            x0 = int(ux + u * (0.5125 + i * 0.0655)); x1 = int(x0 + u * 0.016)
            y0, y1 = int(H * (yc - 0.013)), int(H * (yc + 0.013))
            px = [im.getpixel((x, y))[:3] for x in range(x0, x1, 2) for y in range(y0, y1, 2)]
            white = sum(1 for p in px if min(p) >= 0xd8 and max(p) - min(p) <= 0x22)
            colour = sum(1 for p in px if max(p) >= 0xb0 and max(p) - min(p) >= 0x38)
            kinds.append('ai' if white > 3 * colour and white > 0.05 * len(px)
                         else 'player' if colour > 3 * white and colour > 0.05 * len(px) else None)
            c = ux + u * (0.5375 + i * 0.0655)
            box = (c - u * 0.03, H * (yc + 0.051), c + u * 0.03, H * (yc + 0.077))
            crops[f'n{i}'] = R.prep_text(im, box, k)
            name_px = [im.getpixel((x, y))[:3] for x in range(int(box[0]), int(box[2]), 2) for y in range(int(box[1]), int(box[3]), 2)]
            green = sum(1 for r, g, b in name_px if g >= 0xb0 and g - r >= 0x30 and g - b >= 0x30)
            mine.append(green > 40 and green > 3 * sum(1 for p in name_px if min(p) >= 0xd0))
        names = self.ocr.read(crops, names={f'n{i}': 24 for i in range(4)})
        members = [{'slot': i, 'name': (names.get(f'n{i}') or '').strip(' /|'), 'ai': kind == 'ai', 'self': mine[i]}
                   for i, kind in enumerate(kinds) if kind]
        self.set_party([m for m in members if m['name']], now)

    # Interlink's Party screen (since the Whisperwake update, in place of the lobby): the boss on top, then the four
    # slots' name pills - 1P in the middle-left, 2P middle-right, 3P far left, 4P far right - as (x0, y0, x1, y1)
    PARTY_SLOTS = ((0.30, 0.17, 0.44, 0.21), (0.58, 0.185, 0.70, 0.225), (0.095, 0.222, 0.215, 0.262), (0.815, 0.209, 0.94, 0.249))
    # ...and each slot's lead Aniimo card lower down ('Irisalis / Lv.60 CP:3578'; CP only on your own)
    PARTY_CARDS = ((0.33, 0.548, 0.44, 0.605), (0.598, 0.563, 0.71, 0.62), (0.116, 0.6, 0.228, 0.657), (0.842, 0.587, 0.952, 0.643))
    # the game's AI teammates (its pills don't mark them any more): names seen filling an Interlink party
    INTERLINK_NPCS = {'flora', 'mario', 'leonard', 'tammy'}

    # The Interlink screen (the week's three bosses): each one's stars as 'earned/cap' over its name, left, center, right
    INTERLINK_STARS = ((0.18, 0.50, 0.28, 0.565), (0.44, 0.375, 0.53, 0.43), (0.71, 0.44, 0.78, 0.485))

    def interlink_screen(self, im, now, u, ux, H):
        """The week's star caps from the Interlink screen ('14/20', '36/40', '20/20'): the cap behind each slash, read
        two ways (the number in front picks up the star icon now and then). Saved when it makes sense - left and
        right the same, the center double - so clears follow the game when it raises them."""
        if now - getattr(self, 'interlink_seen', 0) < 30: return
        self.interlink_seen = now; k = u / 2000
        box = lambda a, b, c, d: (ux + u * a, H * b, ux + u * c, H * d)
        crops = {}
        for i, s in enumerate(self.INTERLINK_STARS):
            crops[f'b{i}'] = R.prep_bright(im, box(*s), k); crops[f't{i}'] = R.prep_text(im, box(*s), k)
        t = self.ocr.read(crops)
        caps = []
        for i in range(3):
            found = [int(c) for key in (f'b{i}', f't{i}') for c in re.findall(r'/\s*(\d{2})(?!\d)', t.get(key) or '')]
            caps.append(max(set(found), key=found.count) if found else None)
        side, center = caps[0] or caps[2], caps[1]
        if side and center is None and caps[0] == caps[2]: center = 2 * side       # both sides agree; the center is double
        if not (side and center and (caps[0] in (None, side)) and (caps[2] in (None, side)) and center == 2 * side and 10 <= side <= 60):
            log.info(f'Interlink screen: star caps not clear enough {caps} (OCR {t!r})'); return
        if star_caps() != (side, center):
            log.info(f'Interlink screen: star caps {side} / {center} / {side} (were {star_caps()[0]} / {star_caps()[1]})')
            try: S.write_json(STAR_CAPS, {'side': side, 'center': center, 'cycle': R.active_cycle_id(), 'seen': time.strftime('%Y-%m-%d %H:%M:%S')})
            except OSError: pass

    def party_names(self, im, u, ux, H):
        """The Party screen's four name pills, prepared ({'n0'..'n3': box})."""
        return {f'n{i}': R.prep_bright(im, (ux + u * a, H * b, ux + u * c, H * d), u / 2000) for i, (a, b, c, d) in enumerate(self.PARTY_SLOTS)}

    def party_screen(self, im, now, u, ux, H, names=None):
        """Interlink's Party screen: each slot's name (its pill reads e.g. '2P @ Flora O': the badge, a ready mark, the
        name, the voice icon), and who is who. Every real player's card shows CP and the game's AI teammates' don't
        (with real players in the party "No team available. Cannot chat." still shows, so it says nothing about AI).
        You: your saved name, else the yellow name. Read in the bright-text preparation (the yellow name is lost to the
        white-text one). Feeds the same party as the old prep screen, so the result screen can tell which name is you
        and fix its own misreads. `names`: party_names' pills, already read (names_first)."""
        if now - getattr(self, 'party_seen', 0) < 2: return
        self.party_seen = now; k = u / 2000
        keep_sample(im, 'party')
        box = lambda a, b, c, d: (ux + u * a, H * b, ux + u * c, H * d)
        crops = dict(names[0]) if names else self.party_names(im, u, ux, H)
        crops.update({f'a{i}': R.prep_grey(im, box(*s), k) for i, s in enumerate(self.PARTY_CARDS)})
        crops['boss'] = R.prep_text(im, box(0.45, 0.005, 0.62, 0.04), k)
        t = self.ocr.read(crops, names={f'n{i}': 20 for i in range(4)})       # (prep_bright pads 20 px)
        # (Season Buffs' "Lv. 4" and Challenge's "4/4" are too small and short for OCR to read reliably: left out)
        known = self.character_name; members = []
        def yellow(i):                                     # your own name is yellow (the others' white)
            x0, y0, x1, y1 = (int(v) for v in box(*self.PARTY_SLOTS[i]))
            px = [im.getpixel((x, y))[:3] for x in range(x0, x1, 3) for y in range(y0, y1, 3)]
            # (yellow from (224,192,80) to (240,224,128) by scene; the yellowish 1P badge alone is ~1% of a pill, a name 3.5-5%)
            return sum(1 for r, g, b in px if r >= 0xc8 and g >= 0xa8 and r - b >= 0x50) > 0.025 * len(px)
        for i in range(4):
            got = party_pill(t.get(f'n{i}') or '', i)
            if not got: continue
            slot, name = got
            card = t.get(f'a{i}') or ''
            pet = re.match(r"\s*([A-Za-z][A-Za-z' -]{1,24}?)\s*(?:/|$)", card)
            lv = re.search(r'L\s*v\s*\.?\s*(\d{1,2})', card); cp = re.search(r'CP\W{0,3}(\d{3,6})', card)
            mine = bool(known and same_name(known, name)) or (not known and yellow(i))
            # a card read that missed (an effect over it) keeps the last read of that slot
            old = next((m for m in (self.party or {}).get('members', []) if m['slot'] == slot and m['name'] == name), {})
            members.append({'slot': slot, 'name': name, 'self': mine, 'pill': i,
                            # AI: no CP on the card (a card that didn't read falls back to the NPCs' names)
                            'ai': not mine and (not cp if card.strip() else name.casefold() in self.INTERLINK_NPCS),
                            'aniimo': pet.group(1).strip() if pet else old.get('aniimo'),
                            'aniimo_level': int(lv.group(1)) if lv else old.get('aniimo_level'),
                            'cp': int(cp.group(1)) if cp else old.get('cp')})
        boss = self.cycle_boss((t.get('boss') or '').strip(' /|'))
        row = self.stage_first(next((r for r in self.catalog if r['table'] == 'puppet_data' and r['bossTitle_en'] == boss
                                     and int(r['puppetId']) in R.active_cycle_bosses()), None) if boss else None)
        if row: self.prep_boss = (now, row)
        log.info(f"Party screen: OCR {t!r} -> boss {boss or '?'}")
        members = sorted({m['slot']: m for m in members}.values(), key=lambda m: m['slot'])   # 1P..4P, one per badge
        self.set_party(members, now)
        # the screen itself goes with the run (its lineup and the leader's buffs): the last capture before the fight,
        # since anyone can still change their team until it starts. Kept in memory; saved when the result is in.
        if self.party: self.party['picture'] = im
        self.read_party_icons(im, [(m['slot'], m['pill']) for m in members], u, ux, H, now)

    # Your card's team row: your three other Aniimo as pictures, each a slice of the game's head portrait cut by a
    # window left of its skill badge. Shares of the UI width (x) from the card box's left / of the screen height (y)
    # from its top; the portrait's whole square for the matcher. Measured on 4K and 2K Party screens.
    TEAM_PITCH = 0.0503
    TEAM_WINDOW = (-0.030, 0.064, -0.012, 0.100)    # x0, y0, x1, y1 (x from the card's left + pitch * i)
    TEAM_SQUARE = (-0.0345, 0.0472, 0.0405)         # left, top, side (side in UI width)
    # The Season Buffs (bottom center): each icon's blue square, and the N of the "Lv. N" under it
    SEASON_BUFFS = ((0.525, 0.867, 0.550, 0.911), (0.5595, 0.867, 0.5845, 0.911))
    SEASON_LEVEL = (0.0153, 0.900, 0.0273, 0.923)   # x from the square's left
    OFFERING = (0.4338, 0.865, 0.4575, 0.909)       # the Special Offering circle ('+' or '...' when none)

    def load_icons(self):
        """The game's own pictures (icons.py), loaded once in the background: the Party screen's portraits and buffs,
        and the Holo-Battle Sim's Victory screen (the gizmo row, which the Gizmo screen's read needs)."""
        if self.icons_state is not None: return
        self.icons_state = 'loading'
        def load():
            import icons
            self.icons = icons.Icons.load(os.path.join(HERE, 'icons'), getattr(self.args, 'player_log', None))
            self.icons_state = 'ready' if self.icons else 'failed'
            log.info('game icons: ' + (f'{len(self.icons.heads)} Aniimo, {len(self.icons.buffs)} buffs (game build {self.icons.build})'
                                       if self.icons else f'not readable ({icons.Icons.error})'))
        threading.Thread(target=load, daemon=True).start()

    def read_party_icons(self, im, cards, u, ux, H, now):
        """Each player's team (cards: (slot, pill) per player) and the leader's buffs from the Party screen's pictures, matched to the game's own (icons.py), in
        the background (a few seconds a read). Each tile keeps its last three answers; the most common one stands, so a
        team changed before the fight follows within a read or two. Nothing is read until the icons are loaded."""
        self.load_icons()
        if self.icons_state != 'ready' or (self.icon_read and self.icon_read.is_alive()): return
        votes = self.party['votes']                          # this party (it's rebuilt each read, its votes carry on)
        box = lambda x0, y0, x1, y1: (ux + u * x0, H * y0, ux + u * x1, H * y1)
        def read():
            ic, teams, buffs = self.icons, {}, []
            for slot, card in cards:                        # every player's card has the same team row
                a, b = self.PARTY_CARDS[card][:2]; team = teams[slot] = []
                for i in range(3):
                    t = a + self.TEAM_PITCH * i; wx0, wy0, wx1, wy1 = self.TEAM_WINDOW; sx, sy, side = self.TEAM_SQUARE
                    r = ic.head(im, box(t + wx0, b + wy0, t + wx1, b + wy1), (ux + u * (t + sx), H * (b + sy), u * side))
                    # accepted only when clearly ahead (checked on 48 tiles: no wrong answer passes); an Aniimo whose
                    # portrait isn't in the install (Leafy, 30 Oct) scores low against everything and stays unread
                    team.append(r[0] if r and ((r[1] >= 0.55 and r[2] >= 0.15) or (r[1] >= 0.68 and r[2] >= 0.10)) else None)
            for sq in self.SEASON_BUFFS:
                r = ic.buff(im, box(*sq), 'season')
                if not r or r[1] < 0.8 or r[2] < 0.2: continue
                x0, y0, x1, y1 = self.SEASON_LEVEL
                d = ic.digit(im, box(sq[0] + x0, y0, sq[0] + x1, y1))
                level = int(d[0]) if d and d[2] >= 0.08 else None
                ids = ic.buffs_named(r[0], 'season', level) if level else []
                names = [ic.buffs[k]['name'] for k in ic.buffs_named(r[0], 'season')]
                buffs.append({'id': int(ids[0]), 'name': ic.buffs[ids[0]]['name'], 'level': level} if ids else
                             {'name': max(set(names), key=names.count)})
            r = ic.buff(im, box(*self.OFFERING), 'offering', text_below=False)
            if r and r[1] >= 0.8 and r[2] >= 0.15:          # none chosen shows '+' or '...' (up to 0.61 against the icons)
                ids = ic.buffs_named(r[0], 'offering')        # a few offerings share an icon: then only their names go
                names = sorted({ic.buffs[k]['name'] for k in ids})
                buffs.append({'id': int(ids[0]), 'name': names[0]} if len(names) == 1 else {'name': ' or '.join(names)})
            party = self.party
            if not party or party['votes'] is not votes: return          # a new party came in meanwhile
            reads = party.setdefault('team_reads', {})
            def pick(h):                                    # agreed by two of the last three reads (or the only read)
                names = [n for n in h if n]
                n = max(set(names), key=names.count) if names else None
                return n if n and (names.count(n) >= 2 or len(h) == 1) else None
            out = party.setdefault('teams', [None] * 4)    # by slot, 1P..4P
            for slot, team in teams.items():
                hist = reads.setdefault(slot, [collections.deque(maxlen=3) for _ in range(3)])
                for i, name in enumerate(team): hist[i].append(name)
                out[slot] = [pick(h) for h in hist]
            mine = next((m['slot'] for m in party['members'] if m['self']), None)
            if mine is not None: party['team'] = out[mine]
            if buffs or 'buffs' not in party: party['buffs'] = buffs
            log.info(f"Party screen icons: teams {out}, buffs {party.get('buffs')}")
        self.icon_read = threading.Thread(target=read, daemon=True); self.icon_read.start()

    def set_party(self, members, now):
        self.screen_settled('party', now)
        """This run's party (from the old prep screen or the Party screen), each slot's name voted over a few reads."""
        if not members: return
        # the same party is read every few seconds: keep each slot's most common reading (OCR slips now and then)
        same = self.party and now - self.party['at'] < 900 and \
            [m['slot'] for m in self.party['members']] == [m['slot'] for m in members]
        votes = self.party['votes'] if same else {}
        for m in members: votes.setdefault(m['slot'], collections.Counter())[m['name']] += 1
        for m in members: m['name'] = votes[m['slot']].most_common(1)[0][0]
        counted = self.party.get('counted', set()) if same else set()
        for m in members:
            if m['self'] or m['ai']: continue
            clear = votes[m['slot']][m['name']] >= 3
            m['name'] = TEAM.steady(m['name'])
            if clear and m['slot'] not in counted: TEAM.saw(m['name']); counted.add(m['slot'])
        changed = not same or [(m['name'], m['ai'], m['self']) for m in members] != \
            [(m['name'], m['ai'], m['self']) for m in self.party['members']]
        kept = {k: self.party[k] for k in ('team_reads', 'teams', 'team', 'buffs') if k in self.party} if same else {}
        self.party = {'at': now, 'members': members, 'votes': votes, 'counted': counted, **kept}   # the icon reads carry on too
        if changed:
            log.info('party: ' + ', '.join(f"{m['name']}{' (AI)' if m['ai'] else ''}{' (you)' if m['self'] else ''}"
                                           + (f" [{m['aniimo']} Lv {m.get('aniimo_level')}{', CP ' + str(m['cp']) if m.get('cp') else ''}]" if m.get('aniimo') else '')
                                           for m in members))

    def overtime_limit(self, now):
        """The run's clock when the leader's Special Offering on the Party screen read as Overtime (its own icon): the
        default plus Overtime's extra minute. For when the clock itself wasn't read as the fight started; else None."""
        recent = self.party and now - self.party['at'] < 900
        picked = recent and any(b.get('name') == 'Overtime' for b in self.party.get('buffs') or [])
        return self.args.duration + OVERTIME_S if picked else None

    def party_for_result(self, now):
        """The party read on the prep screen of this run (recent enough to be the same run), or None."""
        return self.party['members'] if self.party and now - self.party['at'] < 900 else None

    @staticmethod
    def result_names(im, u, ux, H):
        """The result screen's four player-name boxes: prepared for Windows OCR and the Korean model, and grey_crop's
        for the Latin model ({'name0'..'name3': box} each)."""
        crops, grey = {}, {}
        for i in range(4):
            dx = i * 0.1765
            # a soft threshold keeps thin letters (no split or lost first letters): 130 reads every name at 720p,
            # 900p, 1080p, 1440p and 4K; 160 lost a 'T' at 720p and 1080p. As wide as the name pill can get (0.07 cut
            # a player's long name at both ends 25 times on 1 Nov), and below the stat number above it
            # The Latin model reads the box as it is on screen (grey_crop): 195 of 195 labelled names on real 4K result
            # screens, 180 from this preparation; scaled to 720p-1440p 194-195, where it was 158-188
            box = (ux + u * (0.13 + dx), H * 0.618, ux + u * (0.24 + dx), H * 0.655)
            crops[f'name{i}'] = R.prep_text(im, box, u / 2000, thr=130); grey[f'name{i}'] = R.grey_crop(im, box)
        return crops, grey

    def result_screen(self, im, now, u, ux, H, banner, names=None):
        """Fight result right after a boss: Challenge Rating (stars), Challenge Score, Completion Time and one
        highlight stat per player. It doesn't name the boss, so it is tied to the fight the meter just tracked.
        Rows go to logs/results.csv; a 'New Best Record' also updates that boss in scores.csv. `names`: result_names'
        boxes, already read (names_first)."""
        # read again while it's up (SCREEN_READS in all, 0.4 s apart; the Battle Data click often ends it after one or
        # two): each value is the one most reads agree on, and later reads only update the run's numbers
        again = now - self.result_seen < 30
        if again and (getattr(self, 'result_reads', 0) >= SCREEN_READS or now - getattr(self, 'result_last', 0) < 0.45): return
        if not again: self.result_seen = now; self.result_reads = 0; self.result_votes = {}; self.result_shot = None
        self.result_last = now; self.result_reads += 1
        if self.fight: self.end_fight('result screen')
        k = u / 2000; crops = {
            'rating': R.prep_short(im, (ux + u * 0.20, H * 0.33, ux + u * 0.275, H * 0.41), k),     # short: tiled
            'score': R.prep_text(im, (ux + u * 0.43, H * 0.325, ux + u * 0.57, H * 0.415), k, size=1),
            'time': R.prep_short(im, (ux + u * 0.68, H * 0.33, ux + u * 0.79, H * 0.41), k)}
        named, grey = names or self.result_names(im, u, ux, H)
        for i in range(4):
            dx = i * 0.1765
            crops[f'name{i}'] = named[f'name{i}']
            crops[f'stat{i}'] = R.prep_text(im, (ux + u * (0.21 + dx), H * 0.575, ux + u * (0.33 + dx), H * 0.625), k, thr=160)
        t = self.ocr.read(crops, names={f'name{i}': 24 for i in range(4)}, raws=grey)
        num = lambda s: int(re.sub(r'\D', '', s)) if re.search(r'\d', s or '') else None
        V = lambda key, value: vote(self.result_votes, key, value)
        # the score digit by digit against the game's font when it reads clearly (OCR's 1 can come out as a 7)
        reader = self.name_reader()
        sure = reader.number(im, (ux + u * 0.43, H * 0.325, ux + u * 0.57, H * 0.415)) if reader else None
        # the stars too (OCR read 17 as 7 at 900p and 1080p); the gold star after them isn't white, so it's not read
        stars = reader.number(im, (ux + u * 0.20, H * 0.33, ux + u * 0.275, H * 0.41)) if reader else None
        # and the time, less the 's' the screen writes after the seconds (BossRushBattleResultCtrl.initUI): with the 's'
        # the font read none of 60 real result screens, without it all 60, the same at 720p-1440p
        sure_time = reader.number(im, (ux + u * 0.68, H * 0.33, ux + u * 0.79, H * 0.41), unit=True) if reader else None
        rating = V('rating', stars if stars is not None else first_num(t.get('rating')))
        score = V('score', sure if sure is not None else num(t.get('score')))
        secs = V('time', sure_time if sure_time is not None else first_num(t.get('time')))
        f = self.last_fight if self.last_fight and now - self.last_fight[1] < 180 else None
        # the result is the run's: a two-boss fight's under its stage's first boss, its damage both bosses' (run_of)
        run = self.run_of(f[0]) if f else None
        full = clear_stars(run['id'] if f else None)
        # the stars are a number only: the screen prints the run's grade (BossRushBattleResultCtrl.initUI), and no client
        # Lua shows bossrush_grade_data's grade names ("Eliminate"). A two-boss fight's score is left as read
        score = interlink_score(score, rating, full, two_boss=bool(run and run['bosses'] is not None))
        players = []
        for i in range(4):
            dx = i * 0.1765
            name = V(f'name{i}', (t.get(f'name{i}') or '').strip(' /|'))
            parts = [p.strip() for p in (t.get(f'stat{i}') or '').split(' / ') if p.strip()]
            # the value under the stat's label by the game's font first, else OCR (the player card writes the stat floored,
            # digits only: BossRushBattleResultCtrl, the stats from bossrush_fun_data). On 60 real result screens OCR
            # left 10 values unread (7 a lone 1, Targets Defeated; 23 in the first reads saved then) and cut 2 short; the
            # font read every one. 8 digits are more than number() takes, so those stay OCR's
            sure_value = reader.number(im, (ux + u * (0.21 + dx), H * 0.600, ux + u * (0.33 + dx), H * 0.624)) if reader else None
            value = V(f'value{i}', sure_value if sure_value is not None else
                      next((num(p) for p in reversed(parts) if re.fullmatch(r'[\d,.\s]+', p)), None))
            label = V(f'label{i}', canon(next((p for p in parts if not re.fullmatch(r'[\d,.\s]+', p)), ''), RESULT_STATS))
            # a slot is a player when its name, its value or its stat label was read: a slot with its label read but
            # neither of the others dropped out, and that player and their Battle Data card weren't sent (5 sent runs,
            # 31 Oct-1 Nov; 38 of 178 slot reads dropped in the logs had the label). party_slots gives it the Party
            # screen's player of that slot. The label never reads where no player is: 0 of 540 boxes off the cards
            # (beside the row and in the bands above and below it) on 60 saved result screens, 196 of 232 on the
            # cards of the 58 4K ones in a single read
            if name or value is not None or label in RESULT_STATS:
                players.append({'slot': i, 'name': name, 'stat': label, 'value': value})
        party_slots(players, self.party_for_result(now), sure={i for i in range(4) if sure_read(self.ocr, named[f'name{i}'])},
                    you=self.character_name if character_verified(self.uid) else None)
        mine = self.character_name if character_verified(self.uid) else \
            own_name(self.character_name, [p['name'] for p in players], self.party_for_result(now))
        if mine and mine != self.character_name:
            log.info(f'character for uid {self.uid}: {mine}')
            self.character_name = mine
            remember_character(self.uid, mine)
            if self.submitter: self.submitter.link_character(self.uid, mine)
        elif not mine:
            log.info('result screen: not sure which slot is you (a party with other players, and your name not known yet), '
                     'so the character name is left as it is')
        # A clear: finished before the clock ran out with the full rating (clear_stars: 20 left and right, 40 in the
        # center; it was 15 before the Whisperwake update), and the game's time is exact. That is every star under the
        # clock, not proof of a kill: a solo player or the leader can End a run at any stars
        # (QuitBtnUIComponent.onQuitBtnClick; the HUD's own end button shows at full stars), and a wipe ends it too
        # (BossRushConst.LevelEndType: LeaderExit, AllPlayerDie, BossDie, TimeOver). Under the clock with fewer stars
        # is no clear. A two-boss middle fight is one run with one rating, capped as the center (40), and one clock, the
        # first boss's (BossRushInfoUIComponent.setBossRushStar; level graph 78824533 nodes 5-11): its clear is the same
        # test. Whether killing both always shows 40 is for a live run to show; the screen's call stands either way.
        left = (f[0].max_hp - f[0].damage) / f[0].max_hp if f and f[0].max_hp else 0
        if run and run['bosses'] is not None:                          # of the boss on the bar, its own max HP
            left = f'{(f[0].max_hp - f[0].damage) / f[0].boss_hp if f[0].boss_hp else 0:.0%} of {f[0].title}, boss {f[0].order} of 2,'
        else: left = f'{left:.0%}'
        # The clock is the run's own (f.limit: Overtime makes it 4:00), else --duration (180,
        # sys_config_data.BossRushCombatDuration). The screen shows min(the clock, ceil(the run's time))
        # (BossRushBattleResultCtrl.initUI), so a time-up reads the clock exactly. Every star at the clock's end is no
        # kill: a team at full stars keeps hitting for score until time runs out.
        limit = ((f[0].limit or None) if f else None) or self.overtime_limit(now) or self.args.duration
        cleared = bool(f and secs is not None and 0 < secs < limit and f[0].bossrush and (rating or 0) >= full)
        if cleared: f[0].elapsed = secs; log.info(f'result screen: cleared in {secs}s')
        elif f and secs is not None and secs < limit:
            log.info(f'result screen: ended at {secs}s without a clear (rating {rating}, {left} HP left on the meter)')
        # the leader ended it ("The team leader has ended the challenge.", seen as the boss went): the site sorts out
        # what that makes the run (ended early with every star, or not counted)
        ended_by_leader = bool(f and getattr(self, 'leader_ended', None) is f[0])
        # the run's boss: a two-boss fight's first, the one its result screen names (BossRushBattleResultCtrl.initUI)
        boss = run['title'] if f else '(unknown - meter saw no fight)'
        log.info(f'result screen read {self.result_reads}: banner {banner!r}, OCR {t!r} -> {boss}: rating {rating}, score {score}, time {secs}, players {players}')
        if again:                                                  # a later read: only the agreed numbers change
            mark_you(players, self.party_for_result(now), self.character_name)
            if f: self.fight_summary(f[0], result={'new_best': 'best' in banner.lower(), 'rating_stars': rating, 'score': score,
                                                   'time_s': secs, 'cleared': cleared, 'star_cap': full, 'players': players, 'ended_by_leader': ended_by_leader,
                                                   'screenshot': self.result_shot, 'reads': self.result_reads})
            return
        d = os.path.join(HERE, 'logs', 'screens'); os.makedirs(d, exist_ok=True)
        shot = self.picture(im, 'screens', 'result_' + time.strftime('%Y%m%d_%H%M%S'), proof=True); self.result_shot = os.path.relpath(shot, HERE) if shot else None
        path = os.path.join(HERE, 'logs', 'results.csv'); new = not os.path.exists(path)
        with open(path, 'a', newline='', encoding='utf-8') as fh:
            w = csv.writer(fh)
            if new: w.writerow(['time', 'uid', 'cycle', 'boss', 'level', 'new_best', 'rating_stars', 'score', 'time_s',
                                'players', 'meter_damage', 'meter_seconds', 'meter_dps', 'meter_csv', 'screenshot'])
            fi = f[0] if f else None; dmg = run and run['damage']           # the run's damage, over the run's time
            w.writerow([time.strftime('%Y-%m-%d %H:%M:%S'), self.uid, R.active_cycle_id(), boss, fi and fi.level,
                        'best' in banner.lower(), rating, score, secs, json.dumps(players, ensure_ascii=False),
                        dmg, fi and round(fi.elapsed), fi and round(dmg / fi.elapsed) if fi and fi.elapsed > 1 else None,
                        fi and os.path.basename(fi.log.name), os.path.relpath(shot, HERE) if shot else ''])
        party = self.party_for_result(now)
        mark_you(players, party, self.character_name)
        party_shot = None
        if f and party and self.party.get('picture') is not None:   # the Party screen this run started from
            party_shot = self.picture(self.party['picture'], 'screens', 'party_' + time.strftime('%Y%m%d_%H%M%S'), proof=True, fresh=False)
        if f: self.fight_summary(f[0], result={'new_best': 'best' in banner.lower(), 'rating_stars': rating, 'score': score,
                                               'time_s': secs, 'cleared': cleared, 'star_cap': full, 'players': players, 'ended_by_leader': ended_by_leader, 'screenshot': os.path.relpath(shot, HERE) if shot else None},
                                 **({'party': party} if party else {}),
                                 **({'team': self.party.get('team'), 'teams': self.party.get('teams'), 'buffs': self.party.get('buffs')} if party else {}),
                                 **({'party_screenshot': os.path.relpath(party_shot, HERE)} if party_shot else {}))
        if 'best' in banner.lower() and score is not None and f: self.record_score(im, 'new_best', boss, score, rating)
        self.screen_settled('result', time.time())
        self.out('status', f'result recorded: {boss} - score {score}, {rating} stars, {secs}s')
        self.end_sig = screen_sig(im)                           # the note stays only while this screen does

    # -- automatic "View Battle Data": a short session that clicks the button (and, if enabled in settings, holds
    #    keyboard/mouse and hides the controller) until Battle Data opens, the screen changes, or time runs out.
    def start_auto(self, now):
        r = C.load()['results']
        self.auto_until = now + max(HOLD_MIN, min(120, int(r.get("hold_seconds") or 60)))
        # the first click waits RESULT_PAUSE (open_battle_data retries 1 s after last_click), so the result screen is read
        # again and again first; the inputs are held meanwhile
        self.clicks = 0; self.unread = 0; self.last_click = now + RESULT_PAUSE - 1.0; self.hold_started = now; self.bd_hold = False
        if r.get('hold_inputs'): self.hold.start(self.auto_until - now)
        if r.get('hold_controller') and CT.available(): CT.hide_controllers(True)

    def auto_active(self):
        if self.auto_until and time.time() > self.auto_until: self.end_auto('time limit')
        return bool(self.auto_until)

    def end_auto(self, why):
        if not self.auto_until: return
        self.auto_until = 0.0; self.hold.stop()
        if CT.hidden: CT.hide_controllers(False)
        log.info(f'automatic Battle Data ended: {why}')

    def open_battle_data(self, im, u, ux, H):
        """Click the result screen's 'View Battle Data' button, so its per-player stats get captured too.
        Only while the game is the active window and the button really reads 'View Battle Data'; anything unexpected
        ends the session (and any input hold). The cursor goes back where it was."""
        self.last_click = time.time()
        if not wc.is_foreground(self.hwnd):
            self.end_auto('game is not the active window'); return
        box = (ux + u * 0.53, H * 0.875, ux + u * 0.68, H * 0.925)              # dark text on the white button
        c = im.crop(tuple(int(v) for v in box)).convert('L'); kk = 2 * 2000 / u
        c = c.resize((max(1, int(c.width * kk)), max(1, int(c.height * kk))), Image.BICUBIC).point(lambda v: 0 if v < 120 else 255)
        label = self.ocr.read({'b': ImageOps.expand(c, border=20, fill=255)}).get('b', '')
        if 'battle' not in label.lower():
            # the session starts on the screen's first frame, while it's still sliding in: the button isn't drawn yet
            # (it read '' and ended the hold at once, 5 of 11 times). Not ready yet: try again in a second; a screen
            # that really changed ends the session above ('screen changed')
            self.unread += 1
            if self.unread >= 6: self.end_auto(f'button reads {label!r} after {self.unread} tries')
            else: log.info(f'View Battle Data not readable yet ({label!r}), trying again')
            return
        x = self.origin[0] + ux + u * 0.6; y = self.origin[1] + H * 0.9
        self.clicks += 1; log.info(f'auto-click {self.clicks}: View Battle Data at screen ({x:.0f}, {y:.0f})')
        self.hide_end_note()
        wc.click_at(x, y)

    def run_of(self, f):
        """The run a Fight tracked, as the game keeps it: {'id', 'title' (its boss), 'damage', 'max_hp', 'bosses'}.
        A one-boss fight is its own boss, damage and max HP, with bosses None. A two-boss middle fight is one run under
        its stage's bossId, the first boss, whichever boss it ended on: the result screen names that boss alone and the
        run's best is kept under its id (BossRushBattleResultCtrl.initUI, line 60; BossRushMainCtrl.renderBossInfo,
        line 350). Its damage and max HP are both bosses' as far as the run got, and bosses lists each in order: its
        max HP, damage, whether it went down, and its time on the run's clock (from_s / to_s, seconds: from its bar's
        first sight to its last hit seen, as it died, or to the run's end for the last one; the gap between the bosses
        counts to neither) with its DPS over that time. A run first seen at its second boss (the meter started late)
        counts the first one down at its full max HP: the second spawns only on the first one's death (level graph
        78824533, nodes 16-19); its time is unknown (None)."""
        run = {'id': f.row['puppetId'], 'title': f.title, 'damage': f.damage, 'max_hp': f.max_hp, 'bosses': None}
        if not f.bossrush or getattr(f, 'order', None) is None: return run
        bosses = []
        for i, p in enumerate(f.phases):
            to = round(f.elapsed, 1) if i == len(f.phases) - 1 else p['end_s']     # the last one ends with the run
            secs = round(to - p['start_s'], 1) if to is not None and p['start_s'] is not None else None
            bosses.append({'id': p['row']['puppetId'], 'name': p['title'], 'max_hp': p['max_hp'], 'damage': p['damage'],
                           'defeated': p['defeated'], 'from_s': p['start_s'], 'to_s': to, 'seconds': secs,
                           'dps': round(p['damage'] / secs) if secs and secs > 1 else None, 'breaks': p['breaks']})
        first = f.phases[0]; run.update(id=first['row']['puppetId'], title=first['title'], bosses=bosses)
        _, order, other = R.stage_of(first['row']['puppetId'])
        row = next((r for r in self.catalog if r['table'] == 'puppet_data' and r['puppetId'] == other
                    and r['fixed_hp_rate']), None) if order == 2 else None
        if row:
            hp = R.boss_max_hp(row, f.level)
            bosses.insert(0, {'id': other, 'name': row['bossTitle_en'], 'max_hp': hp, 'damage': hp, 'defeated': True,
                              'from_s': None, 'to_s': None, 'seconds': None, 'dps': None, 'breaks': None, 'seen': False})
            run.update(id=other, title=row['bossTitle_en'] or first['title'], damage=f.damage + hp, max_hp=f.max_hp + hp)
        return run

    def fight_summary(self, f, **parts):
        """logs/fights/<fight>.json: the meter's numbers plus whatever game screens belong to this fight
        (result screen, Battle Data), merged as they appear. A two-boss middle fight's is the run's (run_of)."""
        d = os.path.join(HERE, 'logs', 'fights'); os.makedirs(d, exist_ok=True)
        path = os.path.join(d, os.path.splitext(os.path.basename(f.log.name))[0] + '.json')
        try:
            with open(path, encoding='utf-8') as fh: s = json.load(fh)
        except (OSError, ValueError):
            run = self.run_of(f)
            s = {'boss': run['title'], 'puppetId': run['id'], 'level': f.level, 'uid': self.uid, 'cycle': R.active_cycle_id(),
                 'started': time.strftime('%Y-%m-%d %H:%M:%S', time.localtime(f.t0)), 'bossrush': f.bossrush,
                 # the dungeon's own id from the game log: the same for everyone in that run, so two players' apps
                 # sending the run can be told apart from two unrelated runs with the same score
                 'instance': self.space if f.bossrush and self.space and scene_of(self.space) is None else None,
                 'interlink_scene': bool(getattr(f, 'scene_ok', False)),
                 'server': S.game_server(),
                 # the run's damage and DPS over the run's time: a two-boss fight's both bosses (run_of)
                 'meter': {'max_hp': run['max_hp'], 'damage': run['damage'], 'seconds': round(f.elapsed, 1),
                           'dps': round(run['damage'] / f.elapsed) if f.elapsed > 1 else None, 'segment_left': f.seg,
                           'tracked_from_start': f.started_at_top, 'csv': os.path.basename(f.log.name),
                           'time_limit_s': f.limit or self.overtime_limit(time.time()),
                           'breaks': sum(p['breaks'] for p in f.phases[:-1]) + f.brk.breaks,
                           'break_gauge_points': None if not f.brk.size else round(f.brk.size),
                           'break_rate_pct_s': None if f.brk.rate(time.time())[0] is None
                           else round(f.brk.rate(time.time())[0], 2),
                           # a two-boss stage's bosses in order, as far as the run got (run_of)
                           **({'bosses': run['bosses']} if run['bosses'] is not None else {})}}
        s.update(parts)
        S.write_json(path, s, indent=1, ensure_ascii=False)
        # leaderboard: a run goes, once, when it beats one of your bests on this boss this week (see beats_best),
        # with its result screen and Battle Data. Battle Data's own stats count too, so it decides once that's in.
        if self.submitter and s.get('result') and f.bossrush and 'submit' not in s and not s.get('interlink_scene'):
            log.info("leaderboard: the game log didn't confirm an Interlink scene for this fight - kept in your records, not sent")
            s['submit'] = False
            S.write_json(path, s, indent=1, ensure_ascii=False)
        if self.submitter and s.get('result') and f.bossrush and 'submit' not in s:
            # Battle Data in: decided a few seconds later, after more reads of both screens have voted (vote_reads)
            self.pending_run = (path, time.time() + (BATTLE_DATA_SETTLE if s.get('battle_data') else BATTLE_DATA_WAIT))

    def run_to_send(self, f):
        """Is this fight's run waiting for its Battle Data, or going to the leaderboard and not sent yet? (Then its
        Battle Data screenshot is kept as proof.)"""
        path = os.path.join(HERE, 'logs', 'fights', os.path.splitext(os.path.basename(f.log.name))[0] + '.json')
        try:
            with open(path, encoding='utf-8') as fh: s = json.load(fh)
        except (OSError, ValueError): return False
        return s.get('submit', True) is not False and not s.get('sent')

    def beats_best(self, s):
        """Which of your bests on this boss this week this run beats: 'score' (most stars, then highest score), 'clear'
        (fastest clear), and from your own Battle Data card 'dps', 'break', 'taken' and 'heal' (highest). Each is a
        board of its own, so each counts. Kept per character in logs/bests.json; the run becomes the new best of each."""
        res = s.get('result') or {}
        players = res.get('players') or []
        cards = (s.get('battle_data') or {}).get('players') or []
        me = next((i for i, p in enumerate(players) if p.get('self')), 0 if len(players) <= 1 else None)
        card = S.card_of(cards, players[me] if me < len(players) else {}, me) if me is not None else {}
        clear = res.get('time_s') if res.get('cleared') else None
        mine = {'score': [res.get('rating_stars') or 0, res.get('score') or 0],
                'clear': -clear if clear is not None else None,             # higher is better for every key
                'dps': card.get('avg_dps'), 'break': card.get('break'), 'taken': card.get('taken'), 'heal': card.get('heal')}
        # one best per stage: a two-boss fight's under its stage's bossId, the first boss, whichever boss the run ended
        # on. The game keeps it so (BossRushMainCtrl.renderBossInfo, line 350; the result screen's new-best badges
        # compare against the first boss's: BossRushBattleResultCtrl.initUI, line 60)
        boss = s.get('puppetId'); _, order, partner = R.stage_of(boss, s.get('cycle')) if boss else (None, None, None)
        key = f"{self.uid}|{s.get('cycle')}|{partner if order == 2 else boss}"
        path = os.path.join(HERE, 'logs', 'bests.json')
        try:
            with open(path, encoding='utf-8') as fh: bests = json.load(fh)
        except (OSError, ValueError): bests = {}
        old = bests.get(key) if isinstance(bests.get(key), dict) else {}
        better = [k for k, v in mine.items() if v not in (None, 0, [0, 0]) and (old.get(k) is None or v > old[k])]
        if better:
            bests[key] = {**old, **{k: mine[k] for k in better}}
            S.write_json(path, bests)
        return better

    def decide_run(self, path):
        """Battle Data is in (or the wait for it ran out): send the run. Every run goes, for the player's Battle History
        on the site (its proof pictures too); the boards still rank each player's best, and extra.newBests says which of
        your bests this run beat (a run that beats none can't change a ranking)."""
        try:
            with open(path, encoding='utf-8') as fh: s = json.load(fh)
        except (OSError, ValueError): return
        if 'submit' in s: return
        better = self.beats_best(s)
        s['submit'] = True; s['new_bests'] = better
        S.write_json(path, s, indent=1, ensure_ascii=False)
        self.pending_run = None
        log.info(f"leaderboard: sending this run ({'a new best: ' + ', '.join(better) if better else 'no new best; for your Battle History'})")
        self.send_run(path)

    def send_run(self, path):
        """Send a run once: its numbers, the result screen and Battle Data (whichever pictures exist)."""
        try:
            with open(path, encoding='utf-8') as fh: s = json.load(fh)
        except (OSError, ValueError): return
        if s.get('sent'): return
        s['sent'] = True
        S.write_json(path, s, indent=1, ensure_ascii=False)
        self.submitter.enqueue(S.run_payload(s, self.uid), (s.get('result') or {}).get('screenshot'),
                               battle=(s.get('battle_data') or {}).get('screenshot'), party=s.get('party_screenshot'))

    def sent_notice(self, payload, reply):
        """A record reached the leaderboards (the sender's thread): its notice over the game, unless the player turned
        those off (Leaderboards > Sending)."""
        if not (self.args.cfg.get('leaderboard') or {}).get('notify_sent', True): return
        notice = S.sent_notice(payload, reply)
        if notice: self.out('toast', notice)

    def check_pending_run(self, now):
        """A run whose Battle Data never showed up: decide on the result screen alone."""
        if self.pending_run and now > self.pending_run[1]: self.decide_run(self.pending_run[0])

    def record_score(self, im, screen, boss, score, stars):
        """One row in logs/scores.csv per new value (plus a screenshot in logs/screens/), for the leaderboard."""
        if self.last_scores.get(boss) == (score, stars): return
        self.last_scores[boss] = (score, stars)
        d = os.path.join(HERE, 'logs', 'screens'); os.makedirs(d, exist_ok=True)
        shot = self.picture(im, 'screens', f'{screen}_' + time.strftime('%Y%m%d_%H%M%S'),
                            proof=screen in ('challenge_stats', 'battle_prep'))
        path = os.path.join(HERE, 'logs', 'scores.csv'); new = not os.path.exists(path)
        with open(path, 'a', newline='', encoding='utf-8') as f:
            w = csv.writer(f)
            if new: w.writerow(['time', 'uid', 'cycle', 'screen', 'boss', 'score', 'stars', 'screenshot'])
            w.writerow([time.strftime('%Y-%m-%d %H:%M:%S'), self.uid, R.active_cycle_id(), screen, boss,
                        score, stars, os.path.relpath(shot, HERE) if shot else ''])
        self.out('status', f'score recorded: {boss} - best score {score:,}' + (f', {stars} stars' if stars is not None else ''))
        if self.submitter and screen in ('challenge_stats', 'battle_prep'):
            row = None
            if screen == 'battle_prep':
                row = self.week_row(boss)
                # (it also needed the game log's Interlink scene, but since the Whisperwake update the prep screen
                # opens out in the world, before the run's scene: all 7 bests on 30 Oct were refused for that)
                if not row:
                    log.info(f"leaderboard: prep-screen best for {boss!r} not sent - not one of this week's bosses")
                    return
                boss = row['bossTitle_en']
            self.submitter.enqueue(S.score_payload(screen, boss, score, stars, self.uid, R.active_cycle_id(),
                                                   row['puppetId'] if row else None), os.path.relpath(shot, HERE) if shot else None)

    def week_row(self, name):
        """This week's Interlink boss with this name (from the boss list), or None."""
        active = R.active_cycle_bosses()
        return next((r for r in self.catalog if r['table'] == 'puppet_data' and r['bossTitle_en'] == name
                     and int(r['puppetId']) in active), None)

    def game_events(self):
        while not self.events.empty():
            kind, sid, delay = self.events.get()
            if kind != 'uid' or sid != self.uid:
                log.info(f'game log: {kind} {sid or ""}' + (f' (seen {delay:.1f}s after the game wrote it)' if delay is not None else ''))
            if kind == 'enter':
                m = re.match(r'^\d+-(\d+)-\d+$', sid or '')
                if m and not self.uid: self.set_uid(m.group(1))       # someone's home is theirs: only before any UID
                self.space = sid; self.bossrush_space = sid in self.rush_spaces; self.holosim_space = sid in self.sim_spaces
                self.duds = []; self.dud_cand = None
                # the timer check may already have ended this fight; the log only ends it if it is still running
                if self.fight and time.time() - self.fight.created > 5: self.end_fight('scene changed (game log)')
                self.bar = None
            elif kind == 'bossrush':
                sid = sid or self.space
                if sid: self.rush_spaces.add(sid)
                if sid == self.space:
                    self.bossrush_space = True
                    if self.fight and self.fight.bossrush: self.fight.scene_ok = True
            elif kind == 'holosim':
                sid = sid or self.space
                if sid: self.sim_spaces.add(sid)
                if sid == self.space: self.holosim_space = True
                self.load_icons()     # the Victory screen's gizmo row needs them (3 Nov: started mid-Sim, no Party screen)
            elif kind == 'uid': self.set_uid(sid)
            elif kind == 'leave': self.egg_space_at = time.time()   # the fade that follows is a load, not a match's end (egg_fade)

    @staticmethod
    def menu_names(im, u, ux, H):
        """The menu's name box (SCREEN_BOXES 'me'): prepared as the screen read has it, for Windows OCR and the Korean
        model, and grey_crop's for the Latin model ({'me': box} each)."""
        box = (ux + u * 0.657, H * 0.068, ux + u * 0.86, H * 0.104)
        return {'me': R.prep_text(im, box, u / 2000)}, {'me': R.grey_crop(im, box)}

    def menu_name(self, im, u, ux, H, heads, names=None):
        """The menu's name for game_menu. The menu stays still, so a Windows OCR misread of it (the screen read's, heads)
        comes back read after read, and two in a row prove it: the logs have a proven name read with a letter swapped
        or one added 5 times, and another player's 4-letter name read as nothing. With your UID under it, the name
        models read the box instead, as on the result screen (Ocr.read's `names`; the Korean model reads a Hangul name,
        which Windows OCR can't). On 19 labelled menu views (9 openings, 6 names; 1080p, 1440p and 4K, and each scaled
        to 720p-4K) the name proven right goes from 17 to 19 at their own size, 58 to 63 of 76 scaled, and wrong from
        1 and 4 to none; the rest have the UID unread. `names`: menu_names' boxes, already read (names_first)."""
        uid_text = heads.get('uid', '')
        if not self.uid or self.uid not in re.sub(r'\D', '', uid_text.replace('O', '0')):
            return self.game_menu(heads.get('me', ''), uid_text, im, u, ux, H)
        if names is None:
            crops, grey = self.menu_names(im, u, ux, H)
            return self.names_first(self.menu_name, (im, u, ux, H, heads), crops, 24, grey)
        crops, grey = names
        self.game_menu(self.ocr.read(crops, names={'me': 24}, raws=grey).get('me', ''), uid_text, im, u, ux, H)

    def game_menu(self, name, uid_text, im=None, u=0, ux=0, H=0, where='menu', sure=True):
        """The menu opened with Esc or Start shows your name with your UID right under it (and the Profile screen
        shows both too, see profile_screen). When that UID is the one from the game log, the name is yours: the first
        one becomes your character's name for the leaderboards. Once a name is proven, a different read never replaces
        it here (a menu sliding in or an overlay over the card can read "1" or "44inMax"); a renamed character is
        linked again with Link character in the app (link_poll), which the player confirms.
        `sure`: the read is trustworthy on its own (a short name only when the game's font matched it)."""
        digits = re.sub(r'\D', '', (uid_text or '').replace('O', '0'))
        name = re.sub(r'\s+', ' ', (name or '').strip(' /|.'))
        if where == 'menu' and self.uid and self.uid in digits and len(name) <= 2 and im is not None and u:
            name, sure = self.short_name(im, u, ux, H)                  # "0", "Ab": too short for a plain read
            name = name or ''
        # the UID under it vouches for the name, so a one-character name counts too ("0" is a real one); just not a
        # lone bit of punctuation, which is OCR noise
        if not self.uid or self.uid not in digits or not 1 <= len(name) <= 40 or (len(name) == 1 and not name.isalnum()):
            # your UID is on screen but no name came out of it: say so (once a minute), so a report shows why
            if self.uid and self.uid in digits and time.time() - getattr(self, 'menu_miss_logged', 0) > 60:
                self.menu_miss_logged = time.time()
                log.info(f'game {where}: UID {self.uid} on screen but its name was not read (OCR {name!r})')
            return
        if not name[0].isalnum(): sure = False                          # "'MinMax", "-MinMax": a stray mark read too
        if where == 'profile' and self.link and self.link['state'] == 'waiting':
            self.link_read(name, im, u, ux, H); return
        if not sure: return
        known = known_characters().get(self.uid) if character_verified(self.uid) else None
        if known and known != name:
            if time.time() - getattr(self, 'other_name_logged', 0) > 60:
                self.other_name_logged = time.time()
                log.info(f'game {where}: read {name!r} with UID {self.uid}, but your proven name is {known!r} - '
                         'left as it is (to change it: Link character in the app, with your Profile open)')
            return
        if self.menu_proven == (self.uid, name): return                  # once per run of the meter
        # the same name twice in a row first: while the menu slides in, one read can be off
        seen, self.menu_candidate = getattr(self, 'menu_candidate', None), (self.uid, name)
        if seen != (self.uid, name): return
        self.prove_name(name, where, self.proof_crop(im, u, ux, H, where))

    def proof_crop(self, im, u, ux, H, where, suffix=''):
        """The proof the site keeps, saved in logs/characters: the top of the menu (your picture, name and UID), or the
        Profile screen's left column from the name card down to the UID - and nothing else."""
        if im is None or not u: return None
        d = os.path.join(HERE, 'logs', 'characters'); os.makedirs(d, exist_ok=True)
        box = (0.6, 0.045, 0.8, 0.14) if where == 'menu' else (0.015, 0.06, 0.28, 0.95)
        pic = im.crop((int(ux + u * box[0]), int(H * box[1]), int(ux + u * box[2]), int(H * box[3]))).convert('RGB')
        return save_proof(pic, os.path.join(d, f'{self.uid}{suffix}'))

    def prove_name(self, name, where, proof, force=False):
        self.menu_proven = (self.uid, name)
        log.info(f'game {where}: {name} with UID {self.uid} - this is your character')
        self.character_name = name
        remember_character(self.uid, name, verified=True)
        if self.submitter:
            if force: self.submitter.linked = None                        # sent again even if it matches the last
            self.submitter.link_character(self.uid, name, proof=proof)

    # ---- Link character (the app's button): read the Profile screen, then send the name once the player confirms.
    # The app writes character_link.json - {"request": time}, then {"request": time, "confirm": {"uid", "name"}} - and
    # deletes it to cancel. What the meter sees goes in status.json's character.link: waiting / found / done / timeout.
    LINK_FILE = 'character_link.json'
    LINK_SECONDS = 120          # how long a request waits for the Profile screen
    LINK_READS = 3              # Profile reads in a row that must agree

    def link_poll(self, now):
        try:
            with open(os.path.join(HERE, self.LINK_FILE), encoding='utf-8') as fh: req = json.load(fh)
        except (OSError, ValueError):
            req = None
        if not isinstance(req, dict) or not isinstance(req.get('request'), (int, float)):
            if self.link and self.link['state'] in ('waiting', 'found'): log.info('link character: cancelled')
            self.link = None; return
        if not self.link or self.link['request'] != req['request']:
            self.link = {'request': req['request'], 'state': 'waiting', 'reads': [], 'name': None, 'uid': self.uid}
            log.info('link character: asked - waiting for the Profile screen')
        link = self.link
        if link['state'] == 'waiting' and now - req['request'] > self.LINK_SECONDS:
            link['state'] = 'timeout'; log.info('link character: the Profile screen did not show up in time')
        confirm = req.get('confirm')
        if link['state'] == 'found' and isinstance(confirm, dict) and confirm.get('uid') == link['uid'] \
                and confirm.get('name') == link['name'] and link['uid'] == self.uid:
            link['state'] = 'done'
            log.info(f"link character: {link['name']} confirmed by the player")
            self.prove_name(link['name'], 'profile', link['proof'], force=True)

    def link_read(self, name, im, u, ux, H):
        """One Profile screen read while Link character waits: found once LINK_READS reads in a row agree."""
        link = self.link
        link['reads'] = (link['reads'] + [name])[-self.LINK_READS:]
        if len(link['reads']) == self.LINK_READS and len(set(link['reads'])) == 1:
            link.update(state='found', name=name, uid=self.uid, proof=self.proof_crop(im, u, ux, H, 'profile', '-link'))
            log.info(f'link character: read {name!r} with UID {self.uid} - waiting for the player to confirm')

    def short_name(self, im, u, ux, H):
        """(name, sure) for a name of one or two characters (e.g. "0"): Windows OCR drops so short a word, so it's
        matched against the game's own font (namereader) - sure. When that isn't sure, OCR reads it after a drawn
        label ("Player name: 0") at two sizes, and a lone round character is told apart by its shape (a zero is
        narrow, an O round) - not sure: OCR once read a "0" as "1", so that read alone never proves a name."""
        box = (ux + u * 0.657, H * 0.068, ux + u * 0.86, H * 0.104); k = u / 2000
        reader = self.name_reader()
        name = reader.read(im, box, u) if reader else None
        if name: return name, True
        r = self.ocr.read({'a': R.prep_labelled(im, box, k, 'Player name:', size=3),
                           'b': R.prep_labelled(im, box, k, 'Player name:', size=4)})
        a, b = R.after_label(r.get('a'), 'Player name:'), R.after_label(r.get('b'), 'Player name:')
        name = a if a and (a == b or not b) else b if b and not a else a or None
        if name and len(name) == 1 and name in '0Oo':
            aspect = R.ink_aspect(im, box)
            if aspect is not None: name = '0' if aspect < 0.78 else ('o' if name == 'o' else 'O')
        return name, False

    def name_reader(self):
        """The game-font name reader (namereader.py), with the font gamefont.py keeps in the data folder; None when
        the game's font can't be read."""
        if not hasattr(self, '_name_reader'):
            import gamefont, namereader
            font = gamefont.GameFont.load(os.path.join(HERE, 'gamefont'), getattr(self.args, 'player_log', None))
            self._name_reader = namereader.NameReader(font) if font else None
        return self._name_reader

    def profile_screen(self, im, now, u, ux, H):
        """The Profile screen (menu > Profile): your name on your name card at the top left and your UID at the bottom
        left - the same proof as the menu, and easier to read: dark text on a light card. Players pick the card's
        picture, so the name is cut out by its own colour, which also drops the coloured mark after it (and the
        round icon some cards show before it); then read by OCR, or - one to three characters - by the game's font."""
        if now - getattr(self, 'profile_read', 0) < 2: return
        self.profile_read = now
        import namereader
        name_box = (ux + u * 0.092, H * 0.139, ux + u * 0.272, H * 0.185)     # from the avatar's edge
        uid_box = (ux + u * 0.03, H * 0.905, ux + u * 0.17, H * 0.945)
        ink, bg = namereader.ink_colour(im, name_box)
        mask = namereader.drop_icon(namereader.ink_mask(im, name_box, ink, bg)) if ink else None
        reads = {'u': R.prep_text(im, uid_box, u / 2000)}
        if mask is not None:
            k = 2 * 2000 / u
            text = ImageOps.invert(mask.convert('L')).resize((max(1, int(mask.width * k)), max(1, int(mask.height * k))), Image.LANCZOS)
            reads['n'] = ImageOps.expand(text, border=40, fill=255)
        r = self.ocr.read(reads, names={'n': 40})
        name = re.sub(r'\s+', ' ', r.get('n', '')).strip(' /|.')
        # a long name: OCR; a short one only when the game's font matched it (a Korean name always has: hangulnames)
        sure = len(name) > 3 or K.has_hangul(name)
        reader = self.name_reader() if mask is not None and len(name) <= 3 else None
        matched = reader.read(im, name_box, u, px_4k=namereader.PROFILE_PX_4K, mask=mask) if reader else None
        if matched: name, sure = matched, True
        if not name and mask is not None: log.info(f'profile: name not read ({mask.width}x{mask.height} px of text)')
        self.game_menu(name, r.get('u', ''), im, u, ux, H, where='profile', sure=sure)
        if self.uid and self.uid in re.sub(r'\D', '', r.get('u', '').replace('O', '0')):
            self.profile_stats(im, u, ux, H)

    # Collection Progress on the Profile screen: the value right of each label (dark text on a light pill)
    PROFILE_STATS = {'aniimoPoint': (0.112, 0.512, 0.150, 0.545), 'sparklingAniimo': (0.234, 0.512, 0.259, 0.545),
                     'stylePoints': (0.112, 0.560, 0.150, 0.593), 'prismanaAniimo': (0.234, 0.560, 0.259, 0.593)}

    def profile_stats(self, im, u, ux, H):
        """Your own Profile's Collection Progress (Aniimo Point, Sparkling Aniimo, Style Points, Prismana Aniimo), for
        your AniiLogs profile: white numbers on grey, matched digit by digit against the game's font (OCR, after a
        drawn label, when that isn't sure), kept when two reads in a row agree, and sent when they changed. With them
        go your character's gender (the mark after your name) and server (the game's settings name it)."""
        import namereader
        reader, stats, unsure = self.name_reader(), {}, {}
        for key, b in self.PROFILE_STATS.items():
            box = (ux + u * b[0], H * b[1], ux + u * b[2], H * b[3])
            ink, bg = namereader.ink_colour(im, box)
            mask = namereader.ink_mask(im, box, ink, bg) if ink and reader else None
            v = reader.read(im, box, u, max_chars=7, px_4k=namereader.STATS_PX_4K, mask=mask, chars='0123456789') if mask else None
            if v: stats[key] = int(v)
            else: unsure[key] = R.prep_labelled_dark(im, box, u / 2000, 'Count:')
        if unsure:
            for key, text in self.ocr.read(unsure).items():
                v = re.sub(r'(?i)count:?', '', text or '')
                digits = re.findall(r'\d[\d,]*', v)
                if len(digits) != 1: return                                   # all four or nothing
                stats[key] = int(digits[0].replace(',', ''))
        stats = {key: stats[key] for key in self.PROFILE_STATS}
        gender = namereader.gender_mark(im, (ux + u * 0.092, H * 0.139, ux + u * 0.272, H * 0.185))   # the name card
        if gender: stats['gender'] = gender
        server = S.game_server()
        if server: stats['server'] = server
        seen, self.stats_candidate = getattr(self, 'stats_candidate', None), (self.uid, stats)
        if seen != (self.uid, stats) or getattr(self, 'stats_sent', None) == (self.uid, stats): return
        self.stats_sent = (self.uid, stats)
        log.info(f'profile: collection {stats} (UID {self.uid})')
        if self.submitter: self.submitter.character_stats(self.uid, stats)

    def set_uid(self, uid):
        """The player's UID, however the game log gave it: also brings back the name saved for it (characters.json),
        which otherwise only shows again after the next result screen."""
        if not uid or (uid == self.uid and self.character_name): return
        changed = uid != self.uid
        self.uid = uid
        self.character_name = known_characters().get(uid) or (None if changed else self.character_name)
        if self.character_name: log.info(f'character for uid {uid}: {self.character_name} (saved)')
        if self.submitter and self.character_name: self.submitter.link_character(self.uid, self.character_name)

    def hide_end_note(self):
        if self.end_sig is not None: self.out('end_hide', None); self.end_sig = None

    LEADER_NOTE = (0.33, 0.140, 0.67, 0.195)          # "The team leader has ended the challenge.", top center

    def leader_ended_note(self, im, now):
        """Interlink: the game's line when the party leader ends a run early. It shows top center as the boss goes, so it
        is looked for while an Interlink fight's bar is hidden or just gone (about once a second): a quick check for a
        line of white text over the dark strip first, the OCR only then. The run then goes with extra.endedByLeader."""
        if now - getattr(self, 'leader_look', 0) < 1.0 or not getattr(self, 'frame_full', True): return
        f = self.fight or (self.last_fight[0] if self.last_fight else None)     # last_fight: (Fight, ended at)
        if not f or not f.bossrush or (not self.fight and now - getattr(self, 'fight_ended_at', 0) > 20): return
        self.leader_look = now
        W, H = im.size; u = min(W, H * 16 / 9); ux = (W - u) / 2; x0, y0, x1, y1 = self.LEADER_NOTE
        box = (ux + u * x0, H * y0, ux + u * x1, H * y1)
        c = im.crop(tuple(int(v) for v in box)).convert('L'); hist = c.histogram()
        white = sum(hist[230:]) / max(1, c.width * c.height)      # the line: ~4% white; open sky or sand: none or lots
        if not 0.01 <= white <= 0.15 or ImageStat.Stat(c).mean[0] > 140: return
        t = self.ocr.read({'n': R.prep_white(im, box, u / 2000)}).get('n') or ''
        if re.search(r'leader\W+has\W+ended|ended\W+the\W+challenge', t, re.I):
            if getattr(self, 'leader_ended', None) is not f: log.info(f'Interlink: the team leader ended the challenge ({t!r})')
            self.leader_ended = f

    def watch_end_note(self, im):
        """Every frame between fights: hide the end-screen note the moment the screen under it changes (a click,
        Battle Data opening, the screen closing) instead of waiting ~1 s for the next text read to say what's up."""
        if self.end_sig is not None and screen_changed(screen_sig(im), self.end_sig): self.hide_end_note()

    def picture(self, im, folder, name, proof=False, fresh=True):
        """A screen the meter just read. Its data goes into the records (CSV, JSON); the picture is only kept when
        Captures & logs > Keep screenshots says so, or - as leaderboard proof - until it has been sent (the submitter
        deletes it then). None when no picture is kept. A proof of the screen on show now (fresh) is a settled,
        full-size grab (settled_frame), not the frame it was spotted in, which may be mid-animation; a picture kept
        from earlier (the Party screen) is saved as it is."""
        keep = int(self.args.cfg['capture'].get('keep_hours', 0))
        sending = proof and self.submitter and self.submitter.config and self.submitter.config['screenshots']
        if keep == 0 and not sending: return None
        if proof and fresh: im = self.settled_frame(im)
        path = save_picture(im, folder, name, 3840 if sending else 1920)
        if proof: self.proof_with_overlay(im, name)
        return path

    def proof_with_overlay(self, im, name):
        """Next to each proof (the game alone, which is what's sent), a copy with the DPS window drawn where it was on
        screen a moment later (its result note comes up just after the proof), for the player's own troubleshooting:
        with the test pictures (logs/test_frames/<day>/proof_overlay, Captures & logs › Open the test pictures folder;
        kept 7 days like them), never sent. Drawn from the overlay's own image, so it matches the screen."""
        snap = getattr(self, 'overlay_now', None)
        if snap is None or self.args.replay is not None: return
        origin = self.origin
        def later():
            try:
                parts = snap()
                if not parts: return
                copy = im.convert('RGBA')
                for image, x, y in parts: copy.alpha_composite(image, (int(x - origin[0]), int(y - origin[1])))
                d = os.path.join(HERE, 'logs', 'test_frames', time.strftime('%Y%m%d'), 'proof_overlay')
                os.makedirs(d, exist_ok=True)
                copy.convert('RGB').save(os.path.join(d, name + '_overlay.jpg'), quality=92)   # full size, like the test pictures
            except Exception as e: log.info(f'proof copy with the overlay not saved ({e!r})')
        threading.Timer(1.5, later).start()

    def settled_frame(self, im):
        """The screen once it has stopped moving: a fresh whole frame every SETTLE_S until two in a row match (at most
        three looks, about 0.75 s), so a proof isn't caught sliding in or fading. The frame it was spotted in when a
        look fails, or in a replay."""
        if self.args.replay is not None: return im
        # proofs are the game alone: the overlay steps out of captures (poll_tick) before the first look, so nothing of
        # ours is in the picture and nothing is blacked out (Windows 10 shows it in captures the rest of the time)
        self.proof_until = time.time() + 3
        try: wc.shared.set_clean_until((time.time() + 3) * 1000)      # Sanctum Helper and the app's button too
        except Exception: pass
        try: return self._settled(im)
        finally:
            self.proof_until = time.time() + 0.1      # back in captures once the proof is taken
            try: wc.shared.set_clean_until((time.time() + 0.1) * 1000)
            except Exception: pass

    def _settled(self, im):
        last = im; sig = screen_sig(im) if im is not None else None
        for _ in range(3):
            time.sleep(SETTLE_S)
            try: fresh = self.grab(False, False)
            except Exception: fresh = None
            if fresh is None: break
            fsig = screen_sig(fresh)
            last = fresh
            if sig is not None and not screen_changed(fsig, sig, limit=4): break      # two in a row match: settled
            sig = fsig
        return last

    TEST_FRAMES_MAX = 300           # a day's test pictures at most
    TEST_FRAMES_HOURS = 3           # saved for this long after the option is turned on (or the meter starts with it)
    TEST_FRAMES_GAP = {'fight': 5, 'nobar': 10}   # seconds between pictures of one kind (screens: 8)

    def test_frame_due(self, kind, now):
        cap = self.args.cfg['capture']
        if not cap.get('test_frames') or self.args.replay is not None:
            self.test_since = None; return False
        if getattr(self, 'test_since', None) is None:
            self.test_since = now; self.test_last = {}
            d = os.path.join(HERE, 'logs', 'test_frames', time.strftime('%Y%m%d'))
            self.test_count = sum(len(fs) for at, _, fs in os.walk(d) if 'numbers' not in at) if os.path.isdir(d) else 0
            log.info(f'test pictures: saving for {self.TEST_FRAMES_HOURS} hours ({self.test_count} already today)')
        if now - self.test_since > self.TEST_FRAMES_HOURS * 3600 or self.test_count >= self.TEST_FRAMES_MAX: return False
        return now - self.test_last.get(kind, 0) >= self.TEST_FRAMES_GAP.get(kind, 8)

    def screen_up(self, kind, now):
        """A screen the meter read (each kind test_frame gets, but always): when it first showed and how many reads
        saw it. Once it's gone (no read for 3 s), one line: how long it stayed up and how many reads it took, so the
        proof screens' timing is known before capture changes (B7 plan: settle, then grab)."""
        seen = self.screens_up = getattr(self, 'screens_up', None) or {}
        first, last, n, settle = seen.get(kind, (now, now, 0, None))
        seen[kind] = (first, now, n + 1, settle)
        self.screens_gone(now)

    def screen_settled(self, kind, now):
        """The reader got what it needed from this screen (its numbers read): how long after it first showed."""
        seen = getattr(self, 'screens_up', None) or {}
        if kind in seen and seen[kind][3] is None: seen[kind] = seen[kind][:3] + (now - seen[kind][0],)

    def screens_gone(self, now):
        seen = getattr(self, 'screens_up', None) or {}
        for kind, (first, last, n, settle) in list(seen.items()):
            if now - last >= 3:
                log.info(f'screen up: {kind} for {last - first:.1f} s ({n} read{"" if n == 1 else "s"}' + (f', read after {settle:.1f} s' if settle is not None else '') + ')')
                del seen[kind]

    def test_frame(self, im, kind, now):
        """Captures & logs › Save test pictures: whole frames at the player's own resolution, lossless, for the
        accuracy tests (tests/resolutions.py, tests/bar_resolutions.py) - fights every 5 s and each screen the meter
        reads. Saved in the background so the meter never waits; logs/test_frames/<day>/<kind>/."""
        if kind != 'fight': self.screen_up(kind, now)
        if not getattr(self, 'frame_full', True) or not self.test_frame_due(kind, now): return
        self.test_last[kind] = now; self.test_count += 1
        d = os.path.join(HERE, 'logs', 'test_frames', time.strftime('%Y%m%d'), kind); os.makedirs(d, exist_ok=True)
        path = os.path.join(d, f"{time.strftime('%H%M%S')}_{self.test_count:03d}_{im.width}x{im.height}.png")
        threading.Thread(target=lambda pic=im.copy(): pic.save(path, compress_level=1), daemon=True, name='test-frame').start()
        if self.test_count >= self.TEST_FRAMES_MAX: log.info(f'test pictures: {self.TEST_FRAMES_MAX} today, no more')

    def save_end_screens(self, im, now):
        """For 45 s after a fight: a full frame every 3 s to logs/end_screens/<fight>/, so any results / stat screen
        is kept for comparison even if the meter doesn't recognize it (Battle Data is also saved on its own)."""
        a = self.after_fight
        if not a or not self.args.cfg['capture']['end_screens']: return
        if now > a[1]: self.after_fight = None; return
        if now - a[2] < 3: return
        a[2] = now; d = os.path.join(HERE, 'logs', 'end_screens', a[0]); os.makedirs(d, exist_ok=True)
        save_picture(im, os.path.join('end_screens', a[0]), time.strftime('%H%M%S'))

    def save_nobar(self, im, now):
        """Diagnostics: in an Interlink room (game log) but no boss bar found - keep a few full frames to see why."""
        if not self.args.cfg['capture']['nobar_frames'] or not self.bossrush_space: return   # Interlink only, not the lobby
        if now - self.last_nobar < 5 or self.nobar_count >= 30: return
        self.last_nobar = now; self.nobar_count += 1
        d = os.path.join(HERE, 'logs', 'nobar'); name = time.strftime('%Y%m%d_%H%M%S')
        save_picture(im, 'nobar', name)
        with open(os.path.join(d, name + '.txt'), 'w', encoding='utf-8') as fh: fh.write(R.LAST_FAIL[0] + '\n')

    BAR_LOST_S, BAR_LOST_EACH, BAR_LOST_MAX = 60, 12, 60   # after a lost boss bar: this long, this many, this many a session

    def save_bar_lost(self, im, now):
        """Diagnostics anywhere (the overworld too, where save_nobar doesn't look): for a minute after a boss bar was
        lost, the top of the screen every 5 s (logs/bar_lost, with why the bar wasn't found), so a log report shows the
        bar the meter couldn't hold. Only the top part, where the boss bar is."""
        if not self.args.cfg['capture']['nobar_frames'] or self.bossrush_space: return     # Interlink: save_nobar
        if now - getattr(self, 'bar_lost_at', -1e9) > self.BAR_LOST_S: return
        if now - getattr(self, 'bar_lost_last', 0) < 5 or getattr(self, 'bar_lost_n', 0) >= self.BAR_LOST_EACH: return
        if getattr(self, 'bar_lost_total', 0) >= self.BAR_LOST_MAX: return
        self.bar_lost_last = now; self.bar_lost_n += 1; self.bar_lost_total = getattr(self, 'bar_lost_total', 0) + 1
        H = self.frame_size[1] or im.height
        top = im.crop((0, 0, im.width, min(im.height, int(H * TOP_PART))))
        name = time.strftime('%Y%m%d_%H%M%S')
        save_picture(top, 'bar_lost', name)
        try:
            with open(os.path.join(HERE, 'logs', 'bar_lost', name + '.txt'), 'w', encoding='utf-8') as fh: fh.write(R.LAST_FAIL[0] + '\n')
        except OSError: pass

    def save_debug(self, im, bar, now):
        f = self.fight
        if not f or not f.frames or now - self.last_frame < 0.5: return
        self.last_frame = now; s = bar['span']; lb = bar['left_bracket']; yc = bar['yc']
        crop = im.crop((int(lb - 0.02 * s), int(yc - 0.08 * s), int(lb + 1.03 * s), int(yc + 0.03 * s)))
        if crop.width > 1000: crop = crop.resize((1000, int(crop.height * 1000 / crop.width)), Image.BILINEAR)
        crop.save(os.path.join(f.frames, f'{now - f.created:07.2f}.png'))

    def reload_settings(self, now):
        """Settings changed in the app apply without a restart (which would end a fight): read again when the file
        changes. The overlay's and the game window's are set at start, so the app restarts the meter for those."""
        if now - getattr(self, 'cfg_checked', 0) < 1 or self.args.replay: return
        self.cfg_checked = now
        try: stamp = os.stat(C.SETTINGS_FILE).st_mtime
        except OSError: return
        if stamp == getattr(self, 'cfg_stamp', stamp): self.cfg_stamp = stamp; return
        self.cfg_stamp = stamp; cfg = C.load(); a = self.args
        a.cfg = cfg; a.interval = cfg['capture']['look_interval']
        a.no_debug_frames = not cfg['capture']['debug_frames']; a.no_auto_click = not C.automation(cfg, 'auto_open_battle_data')
        log.info('settings changed - applied')

    def kill_sound(self):
        """When the boss kill sound started, if the app heard one in the seconds before the Catch Countdown."""
        if not self.args.cfg.get('audio_cues', {}).get('boss_kill'): return None
        c = wc.audio.cue()
        if not c or getattr(self, 'kill_sound_used', None) == c[0]: return None
        _, at, score = c
        if not KILL_SOUND_WINDOW[0] <= self.catch_seen - at <= KILL_SOUND_WINDOW[1]: return None
        self.kill_sound_used = c[0]
        log.info(f'boss kill sound heard {self.catch_seen - at:.1f}s before the Catch Countdown (match {score:.2f})')
        return at

    def away_note(self, im):
        """Diagnostics for "You're being attacked" (it has never been seen working): every 3 s while you're tabbed out
        during a fight, what the meter gets - a picture or not, from which capture, black or not - and how long ago it
        last saw the boss bar (the alert needs it within 3 s). Read from a "Report a problem"."""
        now = time.time()
        if not self.fight or not self.hwnd or self.args.replay is not None or wc.is_foreground(self.hwnd): return
        if now - getattr(self, 'away_noted', 0) < 3: return
        self.away_noted = now
        if im is None: picture = 'no picture'
        else:
            lo, hi = im.convert('L').reduce(max(1, im.width // 64)).getextrema()
            picture = f"a picture from {wc.shared.source or '?'}" + (' (black)' if hi < 20 else '')
        log.info(f'tabbed out mid-fight ({self.fight.title}): {picture}, boss bar seen {now - self.bar_seen:.1f} s ago')
        # the bar gone from the picture while the fight goes on (seen on Windows 11's window capture): once a fight, the
        # colours along the row it was on and the picture's average, to tell a hidden HUD, a dimmed or blurred screen
        # and a shifted picture apart
        geom = getattr(self.fight, 'bar_geom', None)
        if im is not None and geom and now - self.bar_seen > 1 and getattr(self, 'away_colours', None) is not self.fight:
            self.away_colours = self.fight
            span, yc = geom; x0 = im.width / 2 - span / 2
            row = [im.getpixel((int(x0 + span * k / 8), min(im.height - 1, int(yc)))) for k in range(9)]
            mean = im.convert('RGB').reduce(max(1, im.width // 32)).resize((1, 1), Image.BOX).getpixel((0, 0))
            log.info(f"tabbed out, no bar: colours along its row (y {yc:.0f}) {' '.join('%02x%02x%02x' % p[:3] for p in row)}; "
                     f"picture {im.width}x{im.height}, average {'%02x%02x%02x' % mean[:3]}")

    HIT_HOLD = 3            # a lower HP must be read this many times in a row (0.75 s) to count
    HIT_BIG = 0.02          # a drop of this share of your HP shows the alert by itself...
    HIT_AGAIN = 10          # ...a smaller one only when another drop follows within this many seconds
    HIT_FLIP = 30           # seconds a number flipping between two readings (a misread) is ignored

    def hit_watch(self, now):
        """While you're tabbed out: the HP number on your HP bar (a strip of its own, 4 times a second). A reading counts
        once the same one is read twice in a row; a lower one is a drop once it is read HIT_HOLD times in a row, and
        the overlay shows "You're being attacked" for a drop of HIT_BIG or more, or for two drops within HIT_AGAIN
        seconds. Windows OCR misreads this small number often, and in a still scene (a town) the same misread comes
        back frame after frame: "11,390 -> 11,356" read twice raised the alert at full HP. A misread also tends to
        flip back and forth between the same two numbers, which is never a hit. (The bar's length missed small hits,
        hits while being healed, and flickered at full HP: false alarms standing in a town.)"""
        if self.args.replay is not None or not self.hwnd or wc.is_foreground(self.hwnd) or not self.frame_size[1]:
            self.hp_seen = self.hp_pending = self.hp_drop = None; return
        if now - getattr(self, 'hp_read_at', 0) < 0.25: return
        self.hp_read_at = now
        if getattr(self, 'hp_reader', None) is None: self.hp_reader = wc.SharedCapture(tag=2)
        if getattr(self, 'hp_ocr', None) is None:
            try: self.hp_ocr = R.Ocr()
            except Exception as e: log.warning(f'HP reader did not start: {e!r}'); self.hp_ocr = False
        if not self.hp_ocr: return
        W, H = self.frame_size; u = min(W, H * 16 / 9); ux = (W - u) / 2
        area = ((ux + u * HP_BAR[0]) / W, HP_BAR[1], (ux + u * HP_BAR[2]) / W, HP_BAR[3])
        strip = self.hp_reader.read_area(self.hwnd, area)
        hp = hp_number(self.hp_ocr, strip, u) if strip is not None else None
        # a misread can come twice from one picture ("260/12694"): under a tenth of your HP is taken for one (by then
        # the hits before it have shown the alert anyway)
        if hp is None or hp[0] > hp[1] or hp[0] * 10 < hp[1]: return
        seen = getattr(self, 'hp_seen', None)
        if seen is None or hp[1] != seen[1] or hp[0] >= seen[0]:
            self.hp_drop = None                                    # no drop (or the old number came back: a misread)
            if hp != getattr(self, 'hp_pending', None): self.hp_pending = hp; return    # wait for the same reading again
            if seen and hp != seen: self.hp_moves = getattr(self, 'hp_moves', []) + [(now, seen[0], hp[0])]
            self.hp_seen = hp; return
        drop = self.hp_drop if getattr(self, 'hp_drop', None) and self.hp_drop['hp'] == hp else {'hp': hp, 'n': 0, 'strip': strip}
        drop['n'] += 1; self.hp_drop = drop
        if drop['n'] < self.HIT_HOLD: return
        self.hp_drop = None; self.hp_seen = hp
        moves = [m for m in getattr(self, 'hp_moves', []) if now - m[0] < self.HIT_FLIP]
        flip = any({a, b} == {seen[0], hp[0]} for _, a, b in moves)
        last = getattr(self, 'hp_drop_at', 0)
        self.hp_moves = moves + [(now, seen[0], hp[0])]; self.hp_drop_at = now
        share = (seen[0] - hp[0]) / hp[1]
        if flip or (share < self.HIT_BIG and now - last > self.HIT_AGAIN):
            log.info(f'tabbed out: HP {seen[0]:,} -> {hp[0]:,} of {hp[1]:,} not taken for a hit '
                     f'({"it flips between these two (a misread)" if flip else "a small drop on its own"})')
            return
        if now - getattr(self, 'hit_at', 0) > 5:
            log.info(f'tabbed out: HP {seen[0]:,} -> {hp[0]:,} of {hp[1]:,} ({share:.1%}), "You\'re being attacked" shown')
            try:                                   # what was read, for a report (only the newest 10 are kept)
                save_picture(strip, 'hp_alerts', time.strftime('%Y%m%d_%H%M%S'))
                d = os.path.join(HERE, 'logs', 'hp_alerts')
                for old in sorted(os.listdir(d))[:-10]: os.remove(os.path.join(d, old))
            except OSError: pass
        self.hit_at = now

    def idle(self):
        """Nothing on screen to keep up with: no fight or boss bar, no automatic clicks or holds, not in Interlink (its
        bosses and result screens come quickly) or an Egg Heist run, and no end screens to save. A boss bar is still
        found within a quarter second."""
        a = self.after_fight
        return (self.args.replay is None and not self.fight and self.bar is None and not self.bossrush_space
                and not self.auto_until and not getattr(self, 'sim_tour', None) and not getattr(self, 'egg_hold', None)
                and not (a and time.time() < a[1]) and not self.sanctum_in_run(time.time()))

    def frame_at(self):
        """When the game showed the frame just read (the app's capture time), or None (a replay, or no such time):
        fight readings use it so a fight's times follow the game."""
        t = getattr(wc.shared, 'frame_time', None)
        return t if t and self.args.replay is None and 0 <= time.time() - t < 1.0 else None

    def fight_interval(self):
        """Seconds between frames in a fight: DPS Meter › Meter › frames a second in a fight: 30 (the default since B6)
        or 60 for the finest timing. A player's game stuttered at 4K with the meter at 60 (B5); the bar's HP and the
        damage read the same at 30."""
        cfg = self.args.cfg
        rate = cfg.get('capture', {}).get('fight_rate', 30)
        chosen = 'capture.fight_rate' in (cfg.get('user_set') or [])
        if not (rate == 60 and chosen): rate = 30              # 60 only when picked (it was the default before B6)
        return FIGHT_INTERVAL if rate == 30 else 1 / 60

    def step(self):
        self.reload_settings(time.time())
        # a screen read whose players' names are in now (names_first): handled before anything else, as when the names
        # were read in place, and the frame loop stood still meanwhile
        self.names_due()
        # holds end on time even without a picture (the game closed or minimized during one): checked first
        hold = getattr(self, 'egg_hold', None)
        if hold and time.time() > hold[0]: self.egg_hold_end('time limit')
        self.auto_active()
        # screen capture switched off (the overlay button or the app): end what's open and rest until it's back on
        off = self.args.replay is None and wc.shared.switched_off()
        if off != getattr(self, 'capture_off', False):
            self.capture_off = off
            log.info('screen capture switched ' + ('off' if off else 'on'))
            if off:
                self.end_auto('screen capture switched off')
                if getattr(self, 'egg_hold', None): self.egg_hold_end('screen capture switched off')
                self.hold.stop()
                if CT.hidden: CT.hide_controllers(False)
                if self.fight: self.end_fight('the DPS meter was switched off')
                self.bar = None
        if off:
            self.out('status', 'The DPS meter is off'); return
        # out in the world, keep the app listening for the boss kill sound while a fight is on
        if self.fight and not self.fight.bossrush and self.args.cfg.get('audio_cues', {}).get('boss_kill'):
            wc.audio.want(time.time())
        self.game_events()
        # In a homeland (anyone's, scene 550) nothing is captured for now: it's heavy on the game already, and a
        # player's whole PC lagged there with aLogs running (B4). The game log says when you leave.
        home = self.args.replay is None and scene_of(self.space) == HOMELAND_SCENE
        if home != getattr(self, 'homeland_rest', False):
            self.homeland_rest = home
            log.info('in a homeland: not capturing until you leave' if home else 'left the homeland: capturing again')
        if home:
            self.bar = None; self.out('status', 'In a homeland: the meter is resting'); return
        if not self.names_waiting(): self.check_pending_run(time.time())     # (Battle Data waiting for its names counts)
        # while a fight's bar is locked, the top of the frame is all that's needed (cheaper capture)
        partial = self.fight is not None and self.bar is not None and time.time() - self.bar_seen < 1.0 \
            and self.args.replay is None
        self.band_frame = False
        if partial and not self.text_due(time.time()) and self.frame_size[1]:
            # between text reads only the band from the top down to just under the bar is needed (name, bar, xN,
            # break line): a fraction of the top part, at every resolution
            b = self.bar; band = (b['yc'] + 0.06 * b['span'] + 4) / self.frame_size[1]
            if band < TOP_PART: partial = band; self.band_frame = True
        # a whole test picture now and then (at most one try a second: without a bar found there's nothing to save)
        if partial and self.fight and self.test_frame_due('fight', time.time()) and time.time() - getattr(self, 'test_try', 0) >= 1:
            partial = False; self.test_try = time.time()
        due = (not partial and self.fight is None and self.bar is None and self.args.replay is None) and self.full_frame_due(time.time())
        top_only = not partial and self.fight is None and self.bar is None and self.args.replay is None and not due
        im = self.grab(partial, top_only, self.screen_parts(time.time()) if due == 'screen' else None)
        self.away_note(im)
        self.hit_watch(time.time())
        if im is None:
            self.out('status', 'waiting for the game window' if not self.hwnd else "can't see the game window"); return
        now = time.time()
        if self.fight and self.hwnd and self.args.replay is None and not wc.is_foreground(self.hwnd):
            self.fight.away += min(1.0, now - getattr(self, 'last_step', now))   # time the fight ran while you were tabbed out
        self.last_step = now
        self.top_sig = screen_sig(im.crop((0, 0, im.width, max(1, int(self.frame_size[1] * TOP_PART)))))
        if self.frame_full: self.egg_fade(im, now)        # Egg Heist: hold inputs through the fade to its end screen
        self.sim_check(now)
        if self.bar is None and self.holosim_space: self.sim_gate(im, now)    # Holo-Battle Sim's exit gate (last level)
        # -- keep the bar locked: re-check its brackets, search again only when they are gone
        if self.bar is None or not R.verify_bar(im, self.bar):
            if partial:                                   # searching and end screens need the whole frame
                im = self.grab()
                if im is None: return
            # the spot the bar had last time first (~1 ms), then the whole-frame search (~3 ms): both every frame
            found = R.bar_at(im, self.bar_spots) or R.find_bar(im)
            if found and not self.fight and self.is_dud(im, found): found = None
            if found and not self.plausible_bar(found, im): found = None
            if found and found.get('world') and ((self.fight and self.fight.bossrush) or self.bossrush_space):
                found = R.as_bossrush_bar(found)                    # Interlink at x1: the xN box can be gone
            elif found and not self.bossrush_space and scene_of(self.space) is not None and self.args.replay is None:
                found = R.as_world_bar(found)                       # the open world: a hit flash is no xN box
            if found:
                self.end_auto('boss bar is back')
                old = self.bar
                if old is None or abs(old['span'] - found['span']) > 3 or abs(old['yc'] - found['yc']) > 3:
                    log.info(f"boss bar found: span {found['span']:.0f}px, y {found['yc']:.0f}, frame {im.size[0]}x{im.size[1]}")
                self.bar = found
                self.bar_sample(now)
                spot = {k: found[k] for k in ('yc', 'track_left', 'left_bracket', 'span', 'thickness')}
                spot['frame'] = list(found['frame'])
                if not any(abs(s['yc'] - spot['yc']) <= 2 and abs(s['track_left'] - spot['track_left']) <= 2
                           and s['frame'] == spot['frame'] for s in self.bar_spots):
                    self.bar_spots = [spot] + self.bar_spots[:3]           # the last few spots, newest first
                    try:
                        S.write_json(BAR_SPOT, self.bar_spots)
                    except OSError: pass
            # in Interlink, while the game log still has you in the run, the fight waits longer for its bar: hidden
            # 20 s by an effect or a dropped capture, it split one Minespine run in two and sent the 13 s fragment.
            # So does a two-boss fight waiting for its second boss, either way: nothing forces that boss into combat and
            # the gap's length is unknown (level graph 78824533: no fbForceEnterCombat after node 3)
            elif self.fight and now - self.bar_seen < (BAR_GONE_GRACE * 4 if self.fight.bossrush and (self.bossrush_space or self.fight.partner is not None)
                                                       else BAR_GONE_GRACE):
                # The bar can hide for a while (effects, BREAK, ultimate animations, menus): the fight stays open and
                # continues when the bar comes back. A "Catch Countdown" means the boss was caught.
                self.watch_end_note(im)
                self.leader_ended_note(im, now)
                self.check_battle_data(im, now)                                # end screens, Catch Countdown
                if self.fight and self.catch_seen >= self.bar_seen - 2:
                    self.end_fight('caught (Catch Countdown)'); self.snapshot(now); return
                if self.fight and self.bar:
                    # Often the bar is still there under the effects - and always during a BREAK, whose gold overlay
                    # hides the bar's right end from the finder: keep reading its fill from the locked position. Only
                    # frames starting with live-HP red count; HP never goes up; and a big drop since the last reading
                    # (a BREAK burst, or a red effect over the spot) counts once three readings in a row agree.
                    fill, fill_rgb, edge_rgb = R.measure_fill_any(im, self.bar)
                    r_, g_, b_ = fill_rgb
                    live = r_ >= 225 and 80 <= g_ <= 140 and 75 <= b_ <= 135
                    last = self.fight.last_fill
                    ok = live and fill is not None and (last is None or fill <= last + 0.05)
                    if ok and last is not None and fill < last - 0.3:
                        run = self.hidden_drop if self.hidden_drop and abs(self.hidden_drop[0] - fill) <= 0.02 else (fill, 0, 0.0)
                        counts = now - run[2] >= CONFIRM_GAP
                        self.hidden_drop = (fill, run[1] + counts, now if counts else run[2]); ok = self.hidden_drop[1] >= 3
                    if ok:
                        self.hidden_drop = None
                        self.fight.update(fill, fill_rgb, edge_rgb, None, None, self.grades, self.args.duration, self.frame_at())
                        self.emit_fight(im, self.bar); self.bar_seen = now        # the bar is there, just not findable
                self.snapshot(now); return
            else:
                if self.bar is not None:
                    log.info(f'boss bar lost ({R.LAST_FAIL[0]})')
                    self.bar_lost_at = now; self.bar_lost_n = 0
                self.bar = None
                if self.fight: self.end_fight('boss bar gone')
                if self.sim_tour_step(im, now): self.snapshot(now); return     # Holo-Battle Sim's Gizmo screen
                self.watch_end_note(im)
                self.leader_ended_note(im, now)
                self.check_battle_data(im, now)
                self.save_end_screens(im, now)
                self.save_nobar(im, now)
                self.save_bar_lost(im, now)
                self.out('status', 'waiting for a boss bar')   # why it isn't found goes to meter.log (1 s snapshot)
                self.snapshot(now); return
        bar = self.bar
        if self.fight: self.test_frame(im, 'fight', now)
        fill, fill_rgb, edge_rgb = R.measure_fill_any(im, bar)
        # The brackets check passes on screens that have white marks where the brackets were: Holo-Battle Sim's
        # Victory screen kept a won fight 'open' at 1% (1 Nov), its result never read. HP never comes back, so a spot
        # that reads far fuller than the fight's lowest, in a colour no HP bar has, for BAR_GONE_S, is no bar.
        last = self.fight.last_fill if self.fight else None
        r_, g_, b_ = fill_rgb if fill_rgb else (0, 0, 0)
        if last is not None and last < 0.5 and fill is not None and fill > last + 0.5 and b_ > r_:
            self.not_bar_since = getattr(self, 'not_bar_since', None) or now
            if now - self.not_bar_since >= BAR_GONE_S:
                log.info(f'boss bar lost (the locked spot reads {fill:.0%} in rgb{fill_rgb}, the fight was at {last:.0%})')
                self.not_bar_since = None; self.bar = None
                # a two-boss fight waiting for its second boss stays open: the bar-gone grace decides, as for a hidden bar
                if self.fight.partner is None: self.end_fight('boss bar gone')
                self.snapshot(now); return
            self.snapshot(now); return                     # not a reading of this fight's HP
        self.not_bar_since = None
        self.bar_seen = now
        scale = bar['span'] / R.REF_SPAN
        need_title = self.fight is None or now - self.last_title > self.title_gap()
        timer = None; text = None
        xn_box = R.box_at(bar, R.XN_BOX)
        top = R.xn_top(self.grades)                            # the counter at full HP (x21)
        xn = xn_digits = X.read_counter(im.crop(tuple(int(v) for v in xn_box)), self.xn_tpl, top=top)   # digit shapes, every frame
        if not self.fight: self.instant_start(xn, now)
        if self.dud_check(bar, fill, xn, now):
            self.bar = None; self.out('status', 'waiting for a boss bar'); self.snapshot(now); return
        # Text (timer, title, xN fallback) is read about once a second. Live, a background OCR thread does it so a
        # frame never waits for it; the result is used on the frame after it arrives. Replays read it in place.
        done = self.ocr_bg.take() if self.ocr_bg else None
        if done:
            text, meta = done
            timer = R.parse_timer(text.get('timer'))
            if meta['xn_missing'] and xn is None: xn = R.parse_xn(text.get('xn'), top)
            if meta['title']: self.identify(text.get('title'), xn, text.get('title_red'))
        if self.text_due(now) and not self.band_frame:
            crops = {'timer': R.prep_text(im, R.timer_box(*self.frame_size), scale)}
            if xn is None: crops['xn'] = R.prep_short(im, xn_box, scale)            # OCR as a second opinion
            if need_title:
                crops['title'] = R.prep_text(im, R.box_at(bar, R.TITLE_BOX), scale)
                # an under-levelled player's boss level is red: a second read that keeps red (R.prep_red)
                if R.has_red(im, R.box_at(bar, R.TITLE_BOX), scale):
                    crops['title_red'] = R.prep_red(im, R.box_at(bar, R.TITLE_BOX), scale)
            self.last_text = now
            if need_title: self.last_title = now
            meta = {'xn_missing': xn is None, 'title': need_title}
            if self.ocr_bg: self.ocr_bg.submit(crops, meta)
            else:
                text = self.ocr.read(crops)
                timer = R.parse_timer(text.get('timer'))
                if xn is None: xn = R.parse_xn(text.get('xn'), top)
                if need_title: self.identify(text.get('title'), xn, text.get('title_red'))
        if self.fight:
            self.save_debug(im, bar, now)
            if now - getattr(self, 'brk_at', 0) >= CONFIRM_GAP:   # ~10 readings a second: a refill shows as a jump between two
                self.brk_at = now                                   # readings, sized for that pace
                gauge, kind, _ = R.measure_break(im, bar)
                self.fight.brk.feed(gauge, kind, now)
            bosses = len(self.fight.phases)
            self.fight.update(fill, fill_rgb, edge_rgb, xn, timer, self.grades, self.args.duration, self.frame_at())
            if len(self.fight.phases) != bosses: self.boss_handed_over(f'its bar came back at x{self.fight.seg}')
        snap = dict(bar=bar, fill=fill, fill_rgb=fill_rgb, edge_rgb=edge_rgb, xn_digits=xn_digits, xn_ocr=xn)
        if text: snap['ocr'] = text
        self.snapshot(now, **snap)
        if self.fight: self.emit_fight(im, bar)

    def text_due(self, now):
        """Time to read the bar's text (timer, name, xN fallback): about once a second, the name every 8 s (title_gap)."""
        need_title = self.fight is None or now - self.last_title > self.title_gap()
        return (need_title or now - self.last_text >= 1.0) and not (self.ocr_bg and self.ocr_bg.busy)

    def plausible_bar(self, found, im):
        """False positives out: a boss bar spans at least ~28% of the game's 16:9 width (Alphas and Interlink alike), so
        shapes under 20% are something else; and during a fight only the fight's own bar counts (same height, same size
        give or take the BREAK overlay), never another bar-like shape while its bar is hidden."""
        W, H = im.size; u = min(W, H * 16 / 9)
        if found['span'] < 0.2 * u: return False
        f = self.fight; own = getattr(f, 'bar_geom', None) if f else None
        if own and (abs(found['yc'] - own[1]) > max(6, H * 0.004) or abs(found['span'] - own[0]) > own[0] * 0.12):
            if not getattr(f, 'foreign_logged', False):
                f.foreign_logged = True
                log.info(f"ignored a different bar-like shape during the fight (span {found['span']:.0f}px, y {found['yc']:.0f})")
            return False
        return True

    @staticmethod
    def same_spot(a, b): return abs(a['span'] - b['span']) <= 3 and abs(a['yc'] - b['yc']) <= 3

    def dud_check(self, bar, fill, xn, now):
        """A real boss bar names its boss within a second or two. A bar-like shape that has named nothing for 10 s,
        has no xN box and whose fill never moves (menu frames and static red slivers in the lobby, Interlink's rooms
        and Egg Heist) is set aside: no more title reading on it every second and no 'boss bar found' each time it
        flickers. It counts again as soon as its fill moves (a boss getting hit) or after a scene change."""
        c = self.dud_cand
        if self.fight or xn is not None: self.dud_cand = None; return False
        if not c or not self.same_spot(c, bar) or now - c['seen'] > 5                 or (fill is not None and c['fill'] is not None and abs(fill - c['fill']) > 0.01):
            self.dud_cand = {'span': bar['span'], 'yc': bar['yc'], 'fill': fill, 't0': now, 'seen': now}; return False
        c['seen'] = now
        if now - c['t0'] < 10: return False
        self.duds = (self.duds + [c])[-8:]; self.dud_cand = None
        self.bar_spots = [s for s in self.bar_spots if not self.same_spot(s, bar)]
        log.info(f"ignoring a bar-like shape (span {bar['span']:.0f}px, y {bar['yc']:.0f}): no boss name in 10 s "
                 f"and its fill never moved")
        return True

    def is_dud(self, im, found):
        for d in self.duds:
            if not self.same_spot(d, found): continue
            fill = R.measure_fill_any(im, found)[0]
            if fill is not None and d['fill'] is not None and abs(fill - d['fill']) > 0.01:
                self.duds.remove(d)
                log.info(f"the ignored shape at y {found['yc']:.0f} changed - looking at it again"); return False
            return True
        return False

    def emit_fight(self, im, bar):
        f = self.fight; si = f.seg_info or {}
        # an Interlink fight's HUD a few seconds in (the players' icons top left: how many are real), and the whole
        # frame mid-fight, for checking readers against real pictures (logs/samples)
        if f.bossrush and im is not None and getattr(self, 'frame_full', True):
            age = time.time() - f.created
            if 3 <= age < 30: keep_sample(im, 'interlink_start')
            elif 60 <= age < 120: keep_sample(im, 'interlink_mid')
        self.last_screen = (self.origin[0], self.origin[1], self.frame_size[0], self.frame_size[1])
        # max_hp and damage are the run's (a two-boss fight's both bosses); boss_hp is the max HP of the boss on the bar
        self.out('fight', {'title': f.title, 'level': f.level, 'max_hp': f.max_hp, 'boss_hp': f.boss_hp, 'xn': f.seg, 'fill': f.seg_fill,
                           'damage': f.damage, 'elapsed': f.elapsed, 'avg': f.avg, 'recent': f.recent,
                           'boss': f.boss_of(),
                           'bossrush': f.bossrush, 'holosim': self.holosim_space, 'seg_size': si.get('size_hp'), 'seg_left': si.get('left_hp'),
                           'bar': (self.origin[0] + bar['left_bracket'], self.origin[1] + bar['yc'], bar['span']),
                           'break': f.brk.info(time.time()), 'screen': self.last_screen})

    def identify(self, title_text, xn, red_text=None):
        """Match the boss. A running fight keeps its boss unless 3 clean reads in a row say otherwise. red_text: the
        title read with red kept (R.prep_red), for the level when it is red."""
        level, title = R.parse_title_pair(title_text, red_text)
        level = self.args.level or level
        in_world = self.space is not None and scene_of(self.space) is not None   # open world / homeland, per the game log
        bossrush = not in_world and (xn is not None or self.bossrush_space or (self.fight is not None and self.fight.bossrush))
        row, _ = R.pick_boss(self.catalog, title, bossrush, self.args.boss_id) if title and level else (None, 0)
        if not row:
            if not self.fight and title and self.bar is not None:
                self.out('status', f'reading boss name... ("{title}" Lv {level})')
            return
        if self.fight:
            f = self.fight
            if row['puppetId'] == f.row['puppetId'] and level == f.level:
                self.switch_votes = []; return
            if f.partner is not None and row['puppetId'] == f.partner['puppetId']:
                # A two-boss fight's partner (the week's bossId_2 for this stage, R.stage_of): the same run going on,
                # never a new fight. Its name shows only once the first boss is down (level graph 78824533 nodes
                # 16-19), so one clean read will do while the first boss is in its last bars, two otherwise. Both
                # bosses have the stage's level (bossrush_cycle_data bossLv): another level is a misread
                if level != f.level: return
                self.switch_votes.append((row['puppetId'], level)); self.switch_votes = self.switch_votes[-3:]
                need = 1 if f.partner_due() else 2
                if self.switch_votes[-need:] != [(row['puppetId'], level)] * need: return
                f.next_boss(row, f'its name read {title_text!r}')
                self.boss_handed_over(f'title read {title_text!r}, xN {xn}'); return
            if any(p['row']['puppetId'] == row['puppetId'] for p in f.phases[:-1]):
                return             # a boss already down in this run never comes back: a read from before the hand-off
            self.switch_votes.append((row['puppetId'], level)); self.switch_votes = self.switch_votes[-3:]
            if len(self.switch_votes) < 3 or len(set(self.switch_votes)) != 1: return
            self.end_fight('different boss')
        elif bossrush and self.bossrush_space and self.rush_level and not self.args.level and level != self.rush_level:
            # Every Interlink boss has the season's level (rush_level: the last Interlink fight's, Lv 60 in 61 of 62
            # starts to 2 Nov), so another level on a new fight is a misread: 1 Nov's 'Lv.9 Älp14a Scorchhowl' started a
            # Lv 9 fight (8.5M max HP for 37.9M) that ended as 'different boss' 64 s on. Wait for the next read, as the
            # two-boss partner does. Reads come back to back with no fight (~0.1 s apart), so a few alike are one frame's
            # misread: only the same level read alone for LEVEL_CONFIRM seconds (no gap over 2 s) is a new season's
            # level, and starts it
            odd, now = (row['puppetId'], level), time.time()
            if not self.odd_level or self.odd_level[0] != odd or now - self.odd_level[2] > 2: self.odd_level = (odd, now, now)
            self.odd_level = self.odd_level[:2] + (now,)
            if now - self.odd_level[1] < LEVEL_CONFIRM:
                self.out('status', f'reading boss level... ("{title}" Lv {level}, Interlink bosses are Lv {self.rush_level})')
                return
        self.odd_level = None
        self.start_fight(row, level, title, bossrush, f'title read {title_text!r}, xN {xn}')
        if bossrush: self.rush_level = level

    def boss_handed_over(self, how):
        """A two-boss fight went on to its second boss (Fight.next_boss): the same run, no end and no "Fight over"."""
        f = self.fight; self.switch_votes = []
        self.out('status', f'next boss: {f.title} Lv {f.level}, max HP {f.boss_hp:,} (boss {len(f.phases)} of 2, Interlink)')
        log.info(f"fight details: puppetId {f.row['puppetId']}, boss {len(f.phases)} of 2, {how}, HP bars {f.column}")

    def title_gap(self):
        """Seconds between reads of the name above the bar during a fight: 8, and 2 while a two-boss fight's partner can
        show up any moment (its first boss in its last bars, or the partner's name read once already)."""
        f = self.fight
        if f and f.partner is not None and (f.partner_due() or (self.switch_votes and self.switch_votes[-1][0] == f.partner['puppetId'])):
            return 2
        return 8

    def start_fight(self, row, level, title, bossrush, how):
        hp = R.boss_max_hp(row, level)
        # a two-boss middle fight's first boss has a partner to come (R.stage_of: the stage's bossId_2), from the data
        _, order, partner_id = R.stage_of(row['puppetId']) if bossrush else (None, None, None)
        partner = next((r for r in self.catalog if r['table'] == 'puppet_data' and r['puppetId'] == partner_id
                        and r['fixed_hp_rate']), None) if order == 1 else None
        self.fight = Fight(row['bossTitle_en'] or title, level, row, hp, bossrush, debug_dir=not self.args.no_debug_frames,
                           partner=partner, order=order)
        if DAMAGE_NUMBERS and self.args.cfg['capture'].get('damage_numbers') and self.args.replay is None:   # experimental
            if self.numbers is None:
                import dmgnumbers as N
                self.numbers = N.Recorder(HERE, log)
            self.numbers.fight_started(self.hwnd, self.fight.title)
        self.fight.scene_ok = bossrush and self.bossrush_space    # the game log confirmed an Interlink scene
        if self.bar: self.fight.bar_geom = (self.bar['span'], self.bar['yc'])     # the bar this fight follows
        self.out('status', f'fight started: {self.fight.title} Lv {level}, max HP {hp:,}' + (' (Interlink)' if bossrush else ''))
        log.info(f"fight details: puppetId {row['puppetId']}, {how}, "
                 f"bossrush space {self.bossrush_space}, csv {os.path.basename(self.fight.log.name)}"
                 + (f', HP bars {self.fight.column}' if self.fight.column != R.GRADE_BASE else '')
                 + (f", two-boss fight: {partner['bossTitle_en']} ({partner_id}) comes next" if partner else ''))

    def instant_start(self, xn, now):
        """Interlink: the prep screen just named the boss and every Interlink boss has the same level as the last one,
        so its fight starts on the very first frame of its bar - no waiting for the name above the bar to be read.
        That reading still runs and corrects the boss if it disagrees (identify's three-reads rule)."""
        if self.fight or xn is None or not self.rush_level or not self.prep_boss or now - self.prep_boss[0] > 180: return
        row = self.prep_boss[1]
        self.start_fight(row, self.rush_level, row['bossTitle_en'], True, f'from the prep screen, xN {xn}')
        self.prep_boss = None

    def asked_to_stop(self):
        """The aLogs app asked the meter to stop (its stop event), or the app itself is gone: save and exit now.
        Two cheap kernel checks per look. If the app is gone and the exit somehow hangs, the process ends anyway."""
        k = ctypes.windll.kernel32
        for kind, h in self.stop_handles:
            if k.WaitForSingleObject(h, 0) == 0:                      # WAIT_OBJECT_0: signalled / exited
                log.info('the aLogs app ' + ('asked the meter to stop' if kind == 'event' else 'is gone') + ' - stopping')
                if kind == 'parent':
                    t = threading.Timer(5, lambda: os._exit(0)); t.daemon = True; t.start()
                self.stop = True; return True
        return False

    def presence_tick(self, now):
        """Online / AFK on your profile while the game runs (presence.py): sent when it changes and every 2 minutes
        (the site forgets it after 5), and "offline" once the game closes."""
        if self.args.replay is not None or not self.submitter or not self.uid: return
        last = getattr(self, 'presence_sent', None)
        if not (self.hwnd and ctypes.windll.user32.IsWindow(self.hwnd)):
            if last and last[1] != 'offline':
                self.presence_sent = (self.uid, 'offline', now); self.submitter.presence(self.uid, 'offline')
            return
        if not hasattr(self, 'presence'): self.presence = Presence()
        state = self.presence.state(now, wc.is_foreground(self.hwnd))
        if last and last[:2] == (self.uid, state) and now - last[2] < 120: return
        if not last or last[:2] != (self.uid, state): log.info(f'presence: {state}')
        self.presence_sent = (self.uid, state, now)
        self.submitter.presence(self.uid, state)

    def write_status(self, now):
        """status.json for the desktop app's dashboard, about once a second."""
        if now - self.last_status < 1.0: return
        self.last_status = now; f = self.fight; sub = self.submitter
        self.link_poll(now)
        self.presence_tick(now)
        short = self.ocr_not_english(); ocr = short[0][1] if short else self.ocr       # status.json's ocr, below
        C.write_status({
            'time': now, 'pid': os.getpid(), 'status': self.status_text,
            'game_window': bool(self.hwnd), 'capture': wc.shared.source or None,
            'homeland': bool(getattr(self, 'homeland_rest', False)),   # nothing captured there (the app's button too)
            'notices': bool(getattr(self, 'takes_notices', False)),     # Sanctum Helper hands its notices over (NOTICES)
            'frame': list(self.last_size) if self.last_size else None, 'boss_bar': self.bar is not None,
            # boss_of: a two-boss middle fight's boss on the bar, [1, 2] or [2, 2] (None on one-boss fights); damage and
            # max_hp are the run's, both bosses' so far (Fight)
            'fight': f and {'boss': f.title, 'level': f.level, 'segment': f.seg, 'damage': f.damage, 'max_hp': f.max_hp,
                            'seconds': round(f.elapsed), 'avg_dps': round(f.avg), 'recent_dps': round(f.recent),
                            'boss_of': f.boss_of()},
            'character': {'uid': self.uid, 'name': self.character_name, 'verified': character_verified(self.uid),
                          'dispute': self.submitter.dispute if self.submitter else None,
                          'link': self.link and {k: self.link[k] for k in ('request', 'state', 'name', 'uid')}},
            'auto_battle_data': bool(self.auto_until),
            'leaderboard': {'on': bool(sub and sub.config), 'pending': sub.pending() if sub else 0,
                            'last': sub.last_result if sub else None},
            # Windows OCR's language (the meter's own reader's, Ocr.language: null with none, or when the helper
            # doesn't say) and whether it's English: false (no OCR language, another one, or no helper: started false)
            # and the app warns that the game's screens can't be read, with a button that installs English OCR; null
            # (not said): no warning. English added while the meter runs: true within OCR_RECHECK (ocr_recheck), once
            # every helper the meter has reads it; until then the language of the first one without it (ocr_not_english)
            # and switching true once a look has found English (ocr_found): the app then says English OCR is installed
            # and the meter is switching to it, not that the PC has none, and offers no install
            'ocr': {'language': ocr.language or None, 'english': R.ocr_english(ocr.language), 'started': self.ocr.started,
                    'switching': bool(short) and self.ocr_found},
        })

    def ocr_readers(self):
        """The meter's OCR readers, each with whose it is: its own (names_bg reads on it too; NoOcr when its helper
        didn't start), then the bar's, the screens' and the HP number's (hit_watch's, from the first tab-out on) once
        their helper has started (Ocr). Each picked Windows OCR's language as its helper started (Ocr.language)."""
        others = (("the bar's", getattr(self.ocr_bg, 'ocr', None)), ("the screens'", getattr(self.screen_bg, 'ocr', None)),
                  ("the HP number's", getattr(self, 'hp_ocr', None)))
        return [("the meter's own", self.ocr)] + [(whose, o) for whose, o in others if isinstance(o, R.Ocr)]

    def ocr_not_english(self):
        """The meter's OCR readers (ocr_readers) whose helper doesn't read English (ocr_english false), in that order:
        none with English OCR. Not one that doesn't say (None), as ocr_language_note."""
        return [(whose, o) for whose, o in self.ocr_readers() if R.ocr_english(o.language) is False]

    def ocr_recheck(self, now):
        """Without English OCR (status.json's ocr.english false): has Windows got it since? A helper picks its language
        as it starts, so English added in Windows Settings while aLogs ran (the app's warning and meter.log both say
        to) wasn't read until aLogs restarted, and nothing said so (the app's Install English OCR restarts the meter
        itself). Every OCR_RECHECK seconds a helper started for the look says what Windows OCR picks now (10 ms to
        start, on a thread of its own: the frame loop never waits); English, and every helper the meter has restarts
        in place (Ocr.restart: a fight going on keeps going), so the game's screens are read from then on, status.json
        says English and the app's warning goes. Not with NoOcr: its helper didn't start at all, which installing OCR
        doesn't change. Each helper's own language counts (ocr_not_english), not only the meter's own: one whose
        restart failed, or came back without English, is looked at again OCR_RECHECK on (ocr_look)."""
        if not self.ocr.started or not self.ocr_not_english(): return
        if now - self.ocr_checked < OCR_RECHECK: return
        if getattr(self, 'ocr_probe', None) and self.ocr_probe.is_alive(): return
        self.ocr_checked = now
        self.ocr_probe = threading.Thread(target=self.ocr_look, daemon=True, name='ocr-recheck'); self.ocr_probe.start()

    def ocr_look(self):
        """ocr_recheck's look, on its own thread: a helper started now, and when it reads English, every helper the
        meter has without it restarted (its own, which names_bg shares, the bar's, the screens' and the HP number's).
        Then each one's language again: meter.log says English is read only when every one reads it. One whose
        restart raised, or whose new helper has no English either (Windows still adding it, say), is named there (once
        while it stays: ocr_warn), and ocr_recheck looks again OCR_RECHECK on (status.json says not English meanwhile).
        Before, the line said the helpers restarted whatever they read, and the looks stopped once the meter's own read
        English."""
        try: look = R.Ocr()
        except Exception: return
        try: language = look.language
        finally: look.close()
        if self.stop: return
        self.ocr_found = bool(R.ocr_english(language))
        if not self.ocr_found: return
        short = self.ocr_not_english()
        was = short[0][1].language if short else self.ocr.language
        said = {}                            # this look's warnings, by which (ocr_warn)
        for whose, ocr in short:
            if isinstance(ocr, R.Ocr):       # a worker's helper not started yet isn't listed: it starts with English
                try: ocr.restart()
                except Exception as e: said[whose] = f'OCR helper did not restart ({whose}): {e!r}'
        short = self.ocr_not_english()
        if short:
            still = ', '.join(f"{whose}: {o.language or 'none'}" for whose, o in short)
            said['still'] = (f'Windows OCR: English was added while the meter ran, but not every OCR helper restarted '
                             f"with it ({still}), so the game's text isn't all read yet: another look in {OCR_RECHECK} s")
            self.ocr_warn(said, time.time())
            return
        self.ocr_said = {}
        log.info(f"Windows OCR: {self.ocr.language} now (was {was or 'none'}): English was added while the meter ran, "
                 "so its OCR helpers restarted and the game's screens are read")

    def ocr_warn(self, said, now):
        """ocr_look's warnings ({which: line}), in meter.log when the last look didn't give that line (another helper,
        language or error), and again OCR_SAY_AGAIN on while it stays; the looks in between log nothing, though each
        still tries the restart. Before, every look logged them: a restart that kept failing wrote one or two WARNING
        lines a minute all session, pushing the rest out of meter.log (4 x 5 MB, which a report sends)."""
        last, self.ocr_said = self.ocr_said, {}
        for which, line in said.items():
            was = last.get(which)
            if was and was[0] == line and now - was[1] < OCR_SAY_AGAIN: self.ocr_said[which] = was; continue
            log.warning(line); self.ocr_said[which] = (line, now)

    def run(self):
        if self.args.replay is None:                     # this week's bosses from the game data, after each patch
            threading.Thread(target=refresh_cycles, args=(self,), daemon=True, name='game-cycles').start()
            threading.Thread(target=screenshot_cleanup, args=(self,), daemon=True, name='screenshot-cleanup').start()
        while not self.stop:
            t = time.time()
            if self.asked_to_stop(): break
            try:
                t0 = time.perf_counter(); self.step(); self.peak('look', (time.perf_counter() - t0) * 1000)
            except Exception as e:
                log.exception('error in step'); self.out('status', f'error: {e!r} (details in logs/meter.log)')
            self.write_status(time.time())
            if self.args.replay is None: self.ocr_recheck(time.time())
            if self.args.replay is not None and not self.args.replay: break
            interval = self.fight_interval() if self.fight else self.args.interval
            if self.fight and self.args.replay is None:
                # the bar hidden over a second (effects, BREAK, menus): whole frames to find it again and read the end
                # screens; 10 a second finds it as quickly and is a sixth of the copying (B5: 18 whole 4K frames a second)
                if time.time() - self.bar_seen > 1.0: interval = max(interval, 0.1)
                # tabbed out mid-fight: only the "you're being attacked" alert needs the picture - 2 a second
                if self.hwnd and not wc.is_foreground(self.hwnd): interval = max(interval, 0.5)
            if getattr(self, 'capture_off', False): interval = max(interval, AWAY_INTERVAL)   # resting
            if not self.fight and self.hwnd and self.args.replay is None and not wc.is_foreground(self.hwnd):
                interval = max(interval, AWAY_INTERVAL)     # tabbed out between fights: a lighter pace
            elif self.idle(): interval = max(interval, IDLE_INTERVAL)
            if getattr(self, 'homeland_rest', False): interval = max(interval, 1.0)          # only the game log is read
            nap = max(0, interval - (time.time() - t)); woke = time.perf_counter(); time.sleep(nap)
            late = (time.perf_counter() - woke - nap) * 1000
            if late > 100:                                   # Windows let the meter sleep 100+ ms too long: a PC-wide stall
                h = self.hitches = getattr(self, 'hitches', None) or [0, 0.0]
                h[0] += 1; h[1] = max(h[1], late)
        self.end_auto('meter closed')
        if getattr(self, 'egg_hold', None): self.egg_hold_end('meter closed')
        self.hold.stop()
        if CT.hidden: CT.hide_controllers(False)                     # never leave the controller hidden from the game
        self.end_fight('meter closed'); self.ocr.close()
        if self.submitter and getattr(self, 'presence_sent', None) and self.presence_sent[1] != 'offline':
            self.submitter.presence(self.presence_sent[0], 'offline', wait=True)     # aLogs closing: you're offline
        if self.ocr_bg: self.ocr_bg.close()
        if self.screen_bg: self.screen_bg.close()
        if self.names_bg: self.names_bg.close()
        if self.submitter: self.submitter.stop = True
        if self.logw: self.logw.stop = True

# ---------------------------------------------------------------- output
def stop_handles():
    """Handles to wait on for "stop": the app's stop event and the app's own process (both given by the app)."""
    k = ctypes.windll.kernel32; SYNCHRONIZE = 0x00100000; out = []
    k.OpenEventW.restype = k.OpenProcess.restype = ctypes.c_void_p
    k.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
    name = os.environ.get('ALOGS_STOP_EVENT')
    if name:
        h = k.OpenEventW(SYNCHRONIZE, False, name)
        if h: out.append(('event', h))
    try: parent = int(os.environ.get('ALOGS_PARENT') or 0)
    except ValueError: parent = 0
    if parent:
        h = k.OpenProcess(SYNCHRONIZE, False, parent)
        if h: out.append(('parent', h))
    return out

def fmt(n):
    n = float(n)
    return f'{n / 1e6:.2f}M' if n >= 1e6 else f'{n / 1e3:.1f}K' if n >= 1e4 else f'{n:,.0f}'

def boss_row(d):
    """The DPS window's last row on a two-boss middle fight: the boss on the bar, 'Boss 1/2' or 'Boss 2/2' (its 'fight'
    or 'fight_end' message's 'boss': Fight.boss_of). Nothing on a one-boss fight. The Team row above it is the run's
    damage then, both bosses' so far (Fight)."""
    return [(('Boss', f"{d['boss'][0]}/{d['boss'][1]}"), '#ffffff', 0.015)] if d.get('boss') else []

_last_status = [None]
_status_logged = {}
_meter = [None]
def console_out(kind, data):
    if kind == 'status':
        if _meter[0]: _meter[0].status_text = data
        # a status flipping between two messages several times a second ("can't see the game window" / "waiting for
        # a boss bar": 25,000 lines, meter.log rotating every ~14 h) goes to the log once a minute per message
        now = time.time()
        if data != _last_status[0] and now - _status_logged.get(data, 0) >= 60:
            print('\n' + time.strftime('%H:%M:%S'), data, flush=True); log.info(data); _status_logged[data] = now
        _last_status[0] = data
    elif kind == 'fight':                             # the overlay's other messages (fight_end, end_hide) aren't printed
        s = f"x{data['xn']}" if data.get('xn') else ''
        print(f"\r{data['title']} Lv{data['level']} {s} fill {data['fill'] * 100:5.1f}% | dmg {fmt(data['damage'])} | "
              f"avg {fmt(data['avg'])}/s | 5s {fmt(data['recent'])}/s | {data['elapsed']:.0f}s   ", end='', flush=True)

def load_positions():
    try:
        with open(SETTINGS, encoding='utf-8') as f: return json.load(f)
    except (OSError, ValueError): return {}

SETTLE_S = 0.25         # s between looks while waiting for a proof screen to stop moving
OVERLAY_REDRAW = 0.25   # s between redraws of the overlay's numbers (4 a second)
BAR_HIDDEN = 0.5   # s without the game's boss bar before the labels around it are hidden too

def run_overlay(meter, hide):
    """The DPS window (drag to move, right-click to close the meter) and three HP labels attached to the boss bar.
    Positions are remembered in overlay_positions.json."""
    import tkinter as tk, tkinter.font as tkfont
    from PIL import ImageTk
    import gamefont, layered
    BG, FG, DIM = '#14181f', '#e8e6e3', '#a9b1bd'
    show_dps_window = meter.args.cfg['overlay']['dps_window']
    show_hp_labels = meter.args.cfg['overlay']['hp_labels']
    # a dark panel behind the DPS window for readability over bright scenes: 0 (none) to 100 (solid), in %
    BACKDROP = max(0, min(100, int(meter.args.cfg['overlay'].get('backdrop', 0) or 0)))
    q = queue.Queue(); meter.out = lambda k, d: (q.put((k, d)), console_out(k, d))
    meter.takes_notices = True                        # status.json: Sanctum Helper's notices come here (NOTICES)
    pos = load_positions()
    root = tk.Tk(); root.withdraw()
    def save_positions():
        try:
            S.write_json(SETTINGS, pos)
        except OSError: pass
    def quit_all(e=None): save_positions(); root.destroy()
    OFF = '+-32000+-32000'                            # hidden = parked off screen (showing again never takes focus)
    KEY = '#0b0e14'                                   # made transparent; close to the shadow so edges blend
    families = set(tkfont.families(root))
    FACE = next((f for f in ('Bahnschrift SemiBold SemiConden', 'Bahnschrift SemiBold', 'Segoe UI Semibold') if f in families),
                'Segoe UI Semibold')                  # narrow, rounded numerals like the game's HUD font
    # The game's own HUD font, read once from the game install into the data folder (gamefont.py). Loading takes a
    # moment the first time, so it happens in the background; until then (or without it) Bahnschrift stands in.
    GF = {'font': None}
    def load_font():
        GF['font'] = gamefont.GameFont.load(os.path.join(HERE, 'gamefont'), getattr(meter.args, 'player_log', None))
        log.info('overlay font: ' + ('the game\'s own (UI_Font_Europe)' if GF['font']
                                     else f'{FACE} (game font not readable: {gamefont.GameFont.error})'))
        if gamefont.GameFont.trace: log.info(gamefont.GameFont.trace)
    threading.Thread(target=load_font, daemon=True).start()
    KEY_RGB = tuple(int(KEY[i:i + 2], 16) for i in (1, 3, 5))
    rgb = lambda colour: tuple(int(colour[i:i + 2], 16) for i in (1, 3, 5))

    # Over the game only: while another window is in front (you tabbed out), every overlay window stays parked off
    # screen; each remembers where it belongs and comes back when the game is in front again.
    AWAY = {'on': False}; placed = {}
    # Where the DPS window is drawn (screen pixels), for the meter to black out of its pictures of the screen
    # (Meter.mask_own): with that, the DPS window and its notes can stay in screenshots and recordings on Windows 10
    # too. A spot it just left is kept a moment: the picture being read can be a little older than the move.
    meter.own_rects = []; meter.own_in_capture = None       # None: not set yet (poll_tick sets it either way)
    def overlay_now():
        """[(RGBA image, screen x, screen y)] of the DPS window while it's shown (Meter.proof_with_overlay)."""
        p = placed.get(dw)
        return [(p[1], p[2], p[3])] if p and p[0] == 'img' and dps_at.get('shown') else []
    meter.overlay_now = overlay_now
    own = {'cur': {}, 'old': []}
    def own_spot(win, rect):
        """Where each overlay window is (the DPS window, its notes, the HP labels), for Meter.mask_own; a spot just left
        is kept half a second, as the picture being read can be a little older than the move."""
        now = time.time(); prev = own['cur'].get(win)
        if prev == rect: return
        if prev: own['old'].append((*prev, now + 0.5))
        if rect: own['cur'][win] = rect
        else: own['cur'].pop(win, None)
        own['old'] = [r for r in own['old'] if r[4] > now]
        meter.own_rects = [(*r, 0) for r in own['cur'].values()] + own['old']
        publish_own(now)
    def publish_own(now):
        """Our spots for Sanctum Helper and the app to cover in their pictures (and again each second: a stale block
        is taken for a closed meter)."""
        own['published'] = now
        try: wc.shared.publish_overlays(wc.shared.OWNER_METER, [r[:4] for r in meter.own_rects if not r[4] or r[4] > now])
        except Exception: pass
    def present(win, image, x, y):
        """With the game font: the window is this RGBA image, with real per-pixel alpha (layered.py)."""
        placed[win] = ('img', image, x, y)
        own_spot(win, (int(x), int(y), int(x) + image.width, int(y) + image.height))
        if not AWAY['on']: layered.show(layered.top_hwnd(win), image, x, y)
    def place(win, x, y):
        """Without the game font: move the window there (a plain Tk window)."""
        placed[win] = ('geo', x, y)
        win.update_idletasks()
        own_spot(win, (int(x), int(y), int(x) + win.winfo_reqwidth(), int(y) + win.winfo_reqheight()))
        if not AWAY['on']: win.geometry(f'+{int(x)}+{int(y)}')
    def park(win): placed.pop(win, None); own_spot(win, None); win.geometry(OFF)

    def ink(im):
        """(left, right) of the letters' solid part, leaving out the soft shadow (at most ~120 alpha), which spreads
        further at larger sizes. Italic text is measured on its lower half: it leans right above that."""
        a = im.getchannel('A')
        if im.info.get('italic'): a = a.crop((0, a.height // 2, a.width, a.height))
        a = a.point(lambda v: 255 if v > 140 else 0)
        box = a.getbbox()
        return (box[0], box[2]) if box else (0, im.width)

    def compose(lines, H, divider=False, center=False, backdrop=None):
        """The stats as one RGBA image in the game font: lines [(text, colour, size, 'i'/'b')], optional divider.
        A text that is a (label, value) pair is a row: the label grey on the left, the value on the right; with rows,
        every line's right edge lines up on the letters. backdrop: the dark card's strength in % (the Overlay setting
        unless given)."""
        backdrop = BACKDROP if backdrop is None else backdrop
        f = GF['font']; pad = max(3, round(H * (0.011 if backdrop else 0.004))); parts = []
        rows = any(isinstance(line[0], tuple) for line in lines)
        for text, colour, size, *style in lines:
            px = max(11, round(H * size)); style = style[0] if style else ''
            if isinstance(text, tuple):
                parts.append(((f.render(text[0], px, rgb('#aab4c3')), f.render(text[1], px, rgb(colour))), px))
            else: parts.append((f.render(text, px, rgb(colour), italic='i' in style, bold='b' in style), px))
        def span(im): left, right = ink(im); return right - left
        def above(im): return max(p.info.get('base', p.height) for p in im)     # a row: label and value share a baseline
        def tall(im):
            if not isinstance(im, tuple): return im.height
            return above(im) + max(p.height - p.info.get('base', p.height) for p in im)
        gap = round(H * 0.012)
        if rows:          # measured on the ink, so the right edges and the labels' left edges agree to the pixel; at
            #               least as wide as a long reading, so the card doesn't jump as the numbers change
            px0 = max(11, round(H * 0.015))
            least = span(f.render('Last 5s', px0)) + gap + span(f.render('888.8K / 8.88M HP', px0))
            width = max([least] + [span(im[0]) + gap + span(im[1]) if isinstance(im, tuple) else span(im)
                                   for im, _ in parts])
        else: width = max(im.width for im, _ in parts)
        width = max([width] + ([round(H * 0.215)] if divider else []))
        lw = max(1, round(H / 1080)); top = round(H * 0.01) if divider else 0
        height = top + sum(tall(im) + round(px * 0.04) for im, px in parts)
        out = Image.new('RGBA', (width + 2 * pad, height + 2 * pad), (0, 0, 0, 0))
        if backdrop:      # styled like Interlink's stars/timer panel: a dark slate card with well-rounded corners,
            from PIL import ImageDraw  # darkest on the left and easing to ~40% of that on the right
            shape = Image.new('L', out.size, 0)
            ImageDraw.Draw(shape).rounded_rectangle((0, 0, out.width - 1, out.height - 1), radius=round(H * 0.014), fill=255)
            strength = 255 * backdrop / 100
            fade = Image.linear_gradient('L').rotate(90, expand=True).resize(out.size)
            fade = fade.point(lambda v: round(strength * (1 - 0.6 * v / 255)))  # full on the left, 40% on the right
            card = Image.new('RGBA', out.size, (22, 24, 34, 0))
            from PIL import ImageChops
            card.putalpha(ImageChops.multiply(fade, shape)); out.alpha_composite(card)
        if divider:
            from PIL import ImageDraw
            d = ImageDraw.Draw(out)
            d.line((pad, pad + lw, pad + round(H * 0.215), pad + lw), fill=(20, 26, 40, 90), width=lw)
            d.line((pad, pad, pad + round(H * 0.215), pad), fill=(232, 236, 242, 190), width=lw)
        y = pad + top
        for im, px in parts:
            if isinstance(im, tuple):         # a row: the label's ink on the left edge, the value's on the right one
                label, value = im; base = y + above(im)
                out.alpha_composite(label, (pad - ink(label)[0], base - label.info.get('base', label.height)))
                out.alpha_composite(value, (pad + width - ink(value)[1], base - value.info.get('base', value.height)))
            else:
                x = pad + width - ink(im)[1] if rows else pad + ((width - im.width) // 2 if center else 0)
                out.alpha_composite(im, (x, y))
            y += tall(im) + round(px * 0.04)
        return out

    def styled(c, x, y, text, px, fill='#ffffff', anchor='nw', italic=False, bold=False):
        """Game-like text: a thin dark outline and a soft drop shadow under white (or tinted) text."""
        if GF['font']:
            im = GF['font'].render(text, px, tuple(int(fill[i:i + 2], 16) for i in (1, 3, 5)), italic=italic, bold=bold)
            flat = Image.new('RGB', im.size, KEY_RGB); flat.paste(im, mask=im.getchannel('A'))   # edges blend into the key
            photo = ImageTk.PhotoImage(flat, master=c)
            if not hasattr(c, 'photos'): c.photos = []
            c.photos.append(photo)
            return c.create_image(x, y, image=photo, anchor=anchor)
        font = (FACE, -px); o = max(1, round(px / 26)); d = max(1, round(px / 13))
        c.create_text(x, y + d, text=text, font=font, fill='#141a28', anchor=anchor)          # drop shadow
        for dx, dy in ((-o, 0), (o, 0), (0, -o), (0, o)):
            c.create_text(x + dx, y + dy, text=text, font=font, fill='#28324a', anchor=anchor)  # outline
        return c.create_text(x, y, text=text, font=font, fill=fill, anchor=anchor)

    # --- Stats: plain game-styled text, no box. In Interlink (no minimap) it sits in the top-left corner right of Esc, or
    #     under the stars/timer panel (overlay.rush_position);
    #     out in the world right of the minimap. Drag to move (kept as an offset, a fraction of the game height);
    #     right-click closes the meter.
    dw = tk.Toplevel(root); dw.overrideredirect(True); dw.attributes('-topmost', True)
    dw.configure(bg=KEY); dw.attributes('-transparentcolor', KEY); dw.geometry(OFF)
    dc = tk.Canvas(dw, bg=KEY, highlightthickness=0, width=1, height=1); dc.pack()
    dw.dragging = False
    dps_at = {'base': None, 'h': 1, 'shown': False, 'hide_at': None, 'lines': None, 'key': None}
    drag = {}
    def dps_locked(): return bool(meter.args.cfg['overlay'].get('dps_locked', True))
    def dps_start(e):
        if dps_locked(): return
        drag['x'], drag['y'] = e.x_root - dw.winfo_x(), e.y_root - dw.winfo_y(); dw.dragging = True
    def dps_move(e):
        if dw.dragging: dw.geometry(f'+{e.x_root - drag["x"]}+{e.y_root - drag["y"]}')
    def dps_drop(e=None):
        if not dw.dragging: return
        dw.dragging = False
        if dps_at['base']:
            pos[dps_at['key']] = [(dw.winfo_x() - dps_at['base'][0]) / dps_at['h'], (dw.winfo_y() - dps_at['base'][1]) / dps_at['h']]
            save_positions()
    for wdg in (dw, dc):
        wdg.bind('<Button-1>', dps_start); wdg.bind('<B1-Motion>', dps_move)
        wdg.bind('<ButtonRelease-1>', dps_drop); wdg.bind('<Button-3>', quit_all)
    def render_stats(lines, H, divider=False, center=False):
        """lines: [(text, colour, size as a share of the game height, 'i'/'b' style)], under a thin divider when asked
        (out in the world, below the quest list; boss rush has the game's own line under its timer panel)."""
        if GF['font']:
            dps_at['img'] = compose(lines, H, divider, center); return
        dps_at['img'] = None
        dc.delete('all'); dc.photos = []; pad = max(3, round(H * 0.003)); y = pad
        if divider:
            width = round(H * 0.215); lw = max(1, round(H / 1080))
            dc.create_line(pad, y + lw, pad + width, y + lw, fill='#141a28', width=lw)      # shadow
            dc.create_line(pad, y, pad + width, y, fill='#d7dee8', width=lw)
            y += round(H * 0.008)
        rows = any(isinstance(line[0], tuple) for line in lines); right = []    # (items, right edge) to line up
        for text, colour, size, *style in lines:
            px = max(11, round(H * size)); style = style[0] if style else ''
            before = set(dc.find_all())
            if isinstance(text, tuple):       # a row: the label grey on the left, the value moved to the right below
                label = styled(dc, pad, y, text[0], px, '#aab4c3'); before = set(dc.find_all())
                item = styled(dc, dc.bbox(label)[2] + round(H * 0.012), y, text[1], px, colour)
            else: item = styled(dc, pad, y, text, px, colour, italic='i' in style, bold='b' in style)
            if rows: right.append(set(dc.find_all()) - before)
            y = dc.bbox(item)[3] + round(px * 0.12)
        if rows:                              # every value and the big number end on the same right edge
            edge = max(dc.bbox(*items)[2] for items in right)
            for items in right:
                shift = edge - dc.bbox(*items)[2]
                for item in items: dc.move(item, shift, 0)
        box = dc.bbox('all')
        if center:                                    # end screens: each line centred on the widest
            for item in dc.find_all():
                b = dc.bbox(item); dc.move(item, (box[2] - b[2]) // 2, 0)
        if BACKDROP:                                  # without the game font: a solid panel (no partial see-through here)
            dc.tag_lower(dc.create_rectangle(0, 0, box[2] + pad, box[3] + pad, fill=BG, outline=''))
        dc.config(width=box[2] + pad, height=box[3] + pad)
    # The spots (B9), from the game's own HUD layout (its prefabs, as shares of the 16:9 area: x of the width, y of the
    # height), placed clear of where the game shows its own notes and tips:
    #   minimap (0.043-0.144, 0.039-0.218); team panel beside it (0.137-0.264, 0.021-0.069); the AI helper's tips
    #   under that during boss fights (0.136-0.400, 0.141-0.214); boss bar from x 0.302-0.32; one-line system messages
    #   top centre (0.381-0.619, 0.148-0.281); Interlink's stars/timer panel to y 0.326 and its Settle Early button
    #   (0.025-0.194, 0.347-0.391); item and friend toasts on the left from y 0.394-0.455; top right free from x 0.70.
    MINIMAP_RIGHT, BAR_LEFT = 0.195, 0.32     # the party count and menu icons right of the minimap end at ~0.19
    MAP_TOP, MAP_SCALE = 0.069, 0.85          # between the team panel and the AI tips: 0.071 tall, so a little smaller
    def show_dps(screen, bossrush, lines=None, bar=None):
        # the game's HUD is a 16:9 area (centred on ultrawide): spots and text size follow its height, which is the
        # screen's on 16:9 and ultrawide, and the width's 9/16 on 16:10, where the HUD keeps to the top
        ox, oy, W, H = screen; u = min(W, H * 16 / 9); ux = ox + (W - u) / 2; H = u * 9 / 16
        scale = 1.0
        if bossrush and meter.args.cfg['overlay'].get('rush_position') == 'panel':
            base, key = (ux + u * 0.025, oy + H * 0.398), 'dps_offset2_rush'             # under Settle Early
        elif bossrush: base, key = (ux + u * 0.068, oy + H * 0.022), 'dps_offset2_rush_top'  # top left, right of Esc
        elif meter.args.cfg['overlay'].get('world_position') in ('right', 'quests'):
            base, key = (ux + u * 0.705, oy + H * 0.10), 'dps_offset2_right'             # top right, under the buttons
        else:
            # right of the minimap: centred in the gap to the boss bar's left end (where the bar is, else where it
            # usually starts), on a fixed width so it doesn't shift as the numbers change
            scale = MAP_SCALE
            left = ux + u * MINIMAP_RIGHT
            right = bar[0] if bar and left < bar[0] < ux + u * 0.5 else ux + u * BAR_LEFT
            if dps_at.get('bar_left') is None or abs(dps_at['bar_left'] - right) > H * 0.01: dps_at['bar_left'] = right
            width = H * 0.21 * scale
            base, key = (max(left + H * 0.01, (left + dps_at['bar_left']) / 2 - width / 2), oy + H * MAP_TOP), 'dps_offset2_map'
        dps_at['base'], dps_at['h'], dps_at['key'] = base, H, key
        # Overlay › Lock the DPS window: locked, clicks go through it to the game; unlocked, it takes them (to drag it)
        locked = dps_locked()
        if dps_at.get('locked') != locked:
            dps_at['locked'] = locked
            try: layered.click_through(layered.top_hwnd(dw), locked)
            except Exception: pass
        # Overlay › Put the DPS window back: the app bumps dps_reset, and every mode's dragged spot is forgotten
        reset = meter.args.cfg['overlay'].get('dps_reset', 0)
        if reset != pos.get('dps_reset_seen', 0):
            for k in [k for k in pos if k.startswith('dps_offset')]: del pos[k]
            pos['dps_reset_seen'] = reset; save_positions()
        if lines is not None and (lines != dps_at['lines'] or dps_at.get('scale') != scale):
            dps_at['lines'] = lines; dps_at['scale'] = scale; render_stats(lines, H * scale)
        x, y = base; off = pos.get(key)
        if off: x += off[0] * H; y += off[1] * H
        if dw.dragging: x, y = dw.winfo_x(), dw.winfo_y()
        if dps_at.get('img') is not None: present(dw, dps_at['img'], x, y)
        elif not dw.dragging: place(dw, x, y)
        dps_at['shown'] = True; dps_at['hide_at'] = None
    def set_message(text):
        """A note in the stats spot (fight over, result recorded, Battle Data)."""
        if not dps_at['shown'] or not meter.last_screen: return
        H = dps_at['h']                                # the HUD height show_dps placed it by
        lines = [(t, '#ffffff' if i else '#ffd36a', 0.0165 if i else 0.02) for i, t in enumerate(text.split('\n')) if t]
        dps_at['lines'] = lines; render_stats(lines, H * dps_at.get('scale', 1.0))   # the spot's size (smaller by the minimap)
        if dps_at.get('img') is not None: present(dw, dps_at['img'], dw.winfo_x(), dw.winfo_y())
    def hide_dps(): park(dw); dps_at['shown'] = False
    def end_note_result(d):
        # 'result recorded: Omega Sherro - score 11232, 11 stars, 180s' -> 'Result saved / Omega Sherro · 11,232 · 11 stars · 3:00'
        m = re.match(r'result recorded: (.*) - score (\w+), (\w+) stars, (\w+)s$', d)
        if not m: return 'Result saved'
        boss, score, stars, secs = m.groups()
        parts = [boss] + ([f'{int(score):,}'] if score.isdigit() else []) + ([f'{stars} stars'] if stars.isdigit() else [])
        if secs.isdigit(): parts.append(f'{int(secs) // 60}:{int(secs) % 60:02d}')
        return 'Result saved\n' + ' · '.join(parts)

    def end_note_sim(d):
        # 'Holo-Battle Sim recorded: Earth Training - Trial in 644 s (43 gizmos)' -> 'Holo-Battle Sim saved / Earth Training - Trial · 10:44 · 43 gizmos'
        m = re.match(r'Holo-Battle Sim recorded: (.*) in (\d+) s(?: \((.*)\))?$', d)
        if not m: return 'Holo-Battle Sim saved'
        stage, secs, extra = m.groups()
        return 'Holo-Battle Sim saved\n' + ' · '.join([stage, f'{int(secs) // 60}:{int(secs) % 60:02d}'] + ([extra] if extra else []))

    def end_note_battle_data(d):
        # only the players' facts: the game's team DPS next to the meter's (diagnostics stay in meter.log)
        game = re.search(r'game team DPS ([\d,]+)', d); meter = re.search(r'vs meter ([\d,]+)', d)
        if not game: return 'Battle Data saved'
        return 'Battle Data saved\nTeam DPS ' + game.group(1) + (f' · meter {meter.group(1)}' if meter else '')

    def show_end(text, top):
        """A note on an end screen, centred near the bottom where the screen has room (top = share of the height)."""
        if not show_dps_window or not meter.last_screen: return
        ox, oy, W, H = meter.last_screen
        lines = [(t, '#ffd36a' if i == 0 else '#ffffff', 0.02 if i == 0 else 0.0165) for i, t in enumerate(text.split('\n')) if t]
        dps_at['lines'] = lines; render_stats(lines, H, center=True); dw.update_idletasks()
        img = dps_at.get('img')
        width = img.width if img is not None else dc.winfo_reqwidth()
        x, y = int(ox + W / 2 - width / 2), int(oy + H * top)
        if img is not None: present(dw, img, x, y)
        elif not dw.dragging: place(dw, x, y)
        dps_at['shown'] = True; dps_at['end'] = True

    # --- HP labels: white numbers with a thin black outline and a soft dark shadow, attached to the boss bar; the word HP
    #     in the boss bar's colour where the HP ends, BREAK in its state's colour, both with a thick black outline
    #     (positions are fractions of the bracket span from the left bracket / bar centre line, so they follow the bar
    #     at any resolution). Drag one to nudge it - the offset is kept relative to the bar; double-click resets it.
    HP_SPOTS = {                                      # x, y (span fractions), which side of the text sits there, font px
        'pct':   ((R.TRACK[0] + R.TRACK[1]) / 2, 0.0, 'c', 0.021),   # total HP % left: in the middle of the bar
        'bar':   (1.018, 0.0, 'w', 0.024),            # this bar's HP: right of the xN box
        'total': (1.0, 0.022, 'ne', 0.022),           # total HP left: under the bar, flush with its right bracket
        'break': (1.0, 0.049, 'ne', 0.020),           # break gauge: under the total
    }
    pos.pop('hp', None); pos.pop('dps', None)         # old fixed positions
    hp = {}
    def hp_label(name):
        w = tk.Toplevel(root); w.overrideredirect(True); w.attributes('-topmost', True)
        w.configure(bg=KEY); w.attributes('-transparentcolor', KEY); w.geometry(OFF)
        c = tk.Canvas(w, bg=KEY, highlightthickness=0, width=1, height=1); c.pack()
        L = hp[name] = {'win': w, 'c': c, 'base': None, 'span': 1, 'drag': None}
        def start(e): L['drag'] = (e.x_root - w.winfo_x(), e.y_root - w.winfo_y())
        def move(e):
            if L['drag']: w.geometry(f'+{e.x_root - L["drag"][0]}+{e.y_root - L["drag"][1]}')
        def done(e):
            L['drag'] = None
            if L['base']:
                pos['hp_' + name] = [(w.winfo_x() - L['base'][0]) / L['span'], (w.winfo_y() - L['base'][1]) / L['span']]
                save_positions()
        def reset(e): pos.pop('hp_' + name, None); save_positions()
        for x in (w, c):
            x.bind('<Button-1>', start); x.bind('<B1-Motion>', move); x.bind('<ButtonRelease-1>', done)
            x.bind('<Double-Button-1>', reset)
    for n in HP_SPOTS: hp_label(n)

    def with_shadow(img, px):
        """Text with a soft dark shadow (blurred, a little down and right), so white labels stay readable over bright
        effects and snow. Returns the image and how far the text sits inside it."""
        from PIL import ImageFilter
        blur = max(1, round(px * 0.14)); drop = max(1, round(px * 0.06)); edge = 2 * blur + drop
        out = Image.new('RGBA', (img.width + 2 * edge, img.height + 2 * edge), (0, 0, 0, 0))
        mask = Image.new('L', out.size, 0); mask.paste(img.getchannel('A'), (edge + drop, edge + drop))
        mask = mask.filter(ImageFilter.GaussianBlur(blur)).point(lambda v: min(255, v * 2))      # dense near the text
        shade = Image.new('RGBA', out.size, (6, 8, 16, 0)); shade.putalpha(mask)
        out.alpha_composite(shade); out.alpha_composite(img, (edge, edge))
        return out, edge

    def draw(name, text, bar, colour='#ffffff', hp_rgb=None):
        """Text at its spot next to the bar. bar = (left bracket x, bar centre y, bracket span) on screen.
        (Not `hp=`: that hid the labels' own `hp`, and every label failed to draw, from 0.1.28.)"""
        L = hp[name]; c = L['c']; fx, fy, anchor, fs = HP_SPOTS[name]; bx, by, span = bar
        px = max(11, round(span * fs)); pad = max(2, round(px / 13)) + 3
        if GF['font']:
            # the words HP (in the bar's colour where the HP ends, `hp_rgb`) and BREAK / BROKEN (in their line's colour:
            # yellow when broken) with a thick outline; the numbers with a thin one
            lead = (2, hp_rgb or hp_colour(1.0)) if text.startswith('HP ') else                 (len(text.split(' ', 1)[0]), rgb(colour)) if text.startswith(('BREAK', 'BROKEN')) else None
            img = GF['font'].render(text, px, rgb(colour), bold=name == 'pct', stroke=True, lead=lead)
            W, H = img.size; tx, ty = bx + fx * span, by + fy * span
            x, y = {'w': (tx, ty - H / 2), 'ne': (tx - W, ty), 's': (tx - W / 2, ty - H), 'c': (tx - W / 2, ty - H / 2)}[anchor]
            img, edge = with_shadow(img, px); x -= edge; y -= edge     # the text itself stays on its spot
            L['base'], L['span'] = (x, y), span
            off = pos.get('hp_' + name)
            if off: x += off[0] * span; y += off[1] * span
            if L['drag']: x, y = L['win'].winfo_x(), L['win'].winfo_y()
            present(L['win'], img, x, y); return
        c.delete('all'); c.photos = []
        styled(c, pad, pad, text, px, colour, bold=name == 'pct')
        W, H = c.bbox('all')[2] + pad, c.bbox('all')[3] + pad; c.config(width=W, height=H)
        tx, ty = bx + fx * span, by + fy * span
        x, y = {'w': (tx, ty - H / 2), 'ne': (tx - W, ty), 's': (tx - W / 2, ty - H), 'c': (tx - W / 2, ty - H / 2)}[anchor]
        L['base'], L['span'] = (x, y), span
        off = pos.get('hp_' + name)
        if off: x += off[0] * span; y += off[1] * span
        if not L['drag']: place(L['win'], x, y)
    def clear(name):                                  # parked off screen, not withdrawn: showing it never takes focus
        hp[name]['c'].delete('all'); park(hp[name]['win'])
    def hide_hp():
        for n in hp: clear(n)

    for w in [dw] + [L['win'] for L in hp.values()]: w.update_idletasks()
    hidden = {'on': False, 'done': set()}
    def hide_all():
        """Every overlay window (the DPS window, HP labels, result notes, the alert) out of screen captures: the
        player's choice (General Settings › Overlays show in recordings and screenshots, off)."""
        hidden['on'] = True
        for w in root.winfo_children():
            if not isinstance(w, tk.Toplevel) or w in hidden['done']: continue
            try: wc.hide_from_capture(ctypes.windll.user32.GetParent(w.winfo_id()) or w.winfo_id()); hidden['done'].add(w)
            except Exception: pass
    def set_in_capture(on):
        """Every overlay window in screen captures, or out of them (while a proof is taken)."""
        for w in root.winfo_children():
            if isinstance(w, tk.Toplevel): wc.hide_from_capture(ctypes.windll.user32.GetParent(w.winfo_id()) or w.winfo_id(), not on)
    if hide: hide_all()   # on request; otherwise the meter shows up in screenshots and recordings

    def show_hp(d):
        bar = d.get('bar')
        if not bar or not show_hp_labels: hide_hp(); return
        # the HP left on the bar's boss: the run's max HP less its damage is that boss's (Fight), out of its own max HP
        left = max(0, d['max_hp'] - d['damage']); max_hp = d.get('boss_hp') or d['max_hp'] or 1
        # numbers white with a thin black outline; HP and BREAK with a thick one: HP in the bar's colour where the HP
        # ends, BREAK white, yellow when BROKEN, grey-blue refilling, orange gauge (draw, break_text)
        draw('pct', f'{left / max_hp * 100:.1f}%', bar)
        if d['bossrush'] and d['xn'] and d['seg_size']: draw('bar', f"{fmt(d['seg_left'])} / {fmt(d['seg_size'])}", bar)
        else: clear('bar')
        # the HP bar shows this bar's share in Interlink (xN bars), the whole fight's out in the world
        shown = d['seg_left'] / d['seg_size'] if d['bossrush'] and d['xn'] and d['seg_size'] else left / max_hp
        draw('total', f"HP {fmt(left)} / {fmt(max_hp)}", bar, hp_rgb=hp_colour(shown))
        brk = break_text(d.get('break') or {})
        if brk: draw('break', brk[0], bar, brk[1])
        else: clear('break')

    def clock(s): s = max(0, int(round(s))); return f"{s // 60}:{s % 60:02d}"

    def stat_lines(d):
        """The stats: DPS, then rows for the last 5 s, team damage and the fight's time (dashes until the first hit)."""
        hit = d['damage'] > 0
        return [(f"{fmt(d['avg'])} DPS" if hit else '— DPS', '#ffd36a', 0.0235, 'ib'),  # big italic, like the HUD's
                (('Last 5s', f"{fmt(d['recent'])} DPS" if hit else 'waiting...'), '#ffffff', 0.015),
                (('Team', f"{fmt(d['damage'])} / {fmt(d['max_hp'])} HP"), '#ffffff', 0.015),
                (('Time', clock(d['elapsed'])), '#ffffff', 0.015)] + boss_row(d)

    def final_lines(d):
        """The fight's result, left in the stats spot for a few seconds: the final hit is counted by then. A two-boss
        fight's comes once, at the run's end: there is no fight end between its bosses (Fight.next_boss)."""
        return [(f"{fmt(d['avg'])} DPS", '#ffd36a', 0.0235, 'ib'),
                (('Result', f"Defeated {d['title']}" if d['defeated'] and d.get('title')   # the boss's name on the bar
                  else 'Defeated' if d['defeated'] else 'Fight over'), '#ffffff', 0.015),
                (('Team', f"{fmt(d['damage'])} / {fmt(d['max_hp'])} HP"), '#ffffff', 0.015),
                (('Time', clock(d['elapsed'])), '#ffffff', 0.015)] + boss_row(d)

    def break_text(b):
        """(text, colour) for the break gauge line under the boss bar, or None while it can't be read."""
        if b.get('left') is not None or b.get('breaks'):
            rate = b.get('rate_now') if b.get('rate_now') is not None else b.get('rate')
            count = f" · {b['breaks']} break{'s' if b['breaks'] != 1 else ''}" if b.get('breaks') else ''
            if b.get('phase') == 'broken': text, colour = f"BROKEN{count}", '#ffe066'
            elif b.get('phase') == 'refill': text, colour = f"BREAK gauge refilling{count}", '#c8d0dc'
            else:
                size = b.get('size')
                if size:      # break points from the game data: what's left and break points per second
                    text = (f"BREAK {fmt(b['left'] * size)} / {fmt(size)}"
                            + (f" · {fmt(rate * size / 100)}/s" if rate is not None else '') + count)
                else:
                    text = f"BREAK {b['left'] * 100:.0f}% left" + (f" · {rate:.1f}%/s" if rate is not None else '') + count
                colour = '#ffc58a' if b.get('kind') == 'orange' else '#dfe8f5'
            return text, colour
        return None

    # --- "You're being attacked": while the game isn't in front and your HP bar drops (Meter.hit_watch), shown above
    #     every window the way the game shows it in its menus (notice 50003, "Under
    #     attack", in the tips panel's A3 area): text centred ~26% down the screen at ~2.6% of its height, up for 2 s
    #     and again every 5 s, each time with the red flash around the screen's edges the HUD plays when you're hit
    #     (UI_Ani_Hud_Hitted: opacity 0 > 0.5 > 0.9 > 0 over 0.73 s). Click the text to go back to the game.
    aw = tk.Toplevel(root); aw.overrideredirect(True); aw.attributes('-topmost', True)
    aw.configure(bg=KEY); aw.attributes('-transparentcolor', KEY); aw.geometry(OFF)
    ac = tk.Canvas(aw, bg=KEY, highlightthickness=0, width=1, height=1); ac.pack()
    ew = tk.Toplevel(root); ew.overrideredirect(True); ew.attributes('-topmost', True); ew.geometry(OFF)   # edge flash
    alert = {'screen': None, 'shown': False, 'since': None, 'edge': None, 'alpha': -1}
    ALERT_TEXT, ALERT_COLOUR, ALERT_SIZE, ALERT_Y = "You're being attacked in Aniimo", '#ffffff', 0.026, 0.265
    ALERT_SHOW, ALERT_EVERY = 2.0, 5.0
    HIT_CURVE = ((0.0, 0.0), (0.1, 0.5), (0.233, 0.9), (0.733, 0.0))
    def hit_opacity(t):
        for (t0, v0), (t1, v1) in zip(HIT_CURVE, HIT_CURVE[1:]):
            if t0 <= t <= t1: return v0 + (v1 - v0) * (t - t0) / (t1 - t0)
        return 0.0
    def edge_image(W, H):
        """Red around the screen's edges, fading to nothing towards the middle (drawn small, then enlarged)."""
        w, h = max(16, W // 8), max(9, H // 8); depth = 0.1 * min(w, h)
        ramp = lambda n: [round(220 * max(0.0, 1 - i / depth) ** 2.2) for i in range(n)]
        col, row = Image.new('L', (1, h)), Image.new('L', (w, 1))
        col.putdata([max(a, b) for a, b in zip(ramp(h), ramp(h)[::-1])]); row.putdata([max(a, b) for a, b in zip(ramp(w), ramp(w)[::-1])])
        from PIL import ImageChops
        mask = ImageChops.lighter(col.resize((w, h)), row.resize((w, h))).resize((W, H), Image.BILINEAR)
        img = Image.new('RGBA', (W, H), (235, 30, 30, 0)); img.putalpha(mask); return img
    def back_to_game(e=None):
        if meter.hwnd:
            u32 = ctypes.windll.user32
            if u32.IsIconic(meter.hwnd): u32.ShowWindow(meter.hwnd, 9)            # SW_RESTORE
            u32.SetForegroundWindow(meter.hwnd)
    for wdg in (aw, ac): wdg.bind('<Button-1>', back_to_game)
    def draw_alert_text(ox, oy, W, H, hud):
        if GF['font']:
            px = max(14, round(hud * ALERT_SIZE))
            # the game's tip text: white, with a crisp 40% black copy just below and right of it (its font material's
            # underlay), no outline
            text = GF['font'].render(ALERT_TEXT, px, rgb(ALERT_COLOUR), bold=True)
            dx, dy = max(1, round(px * 0.03)), max(1, round(px * 0.07))
            img = Image.new('RGBA', (text.width + dx, text.height + dy), (0, 0, 0, 0))
            under = Image.new('RGBA', text.size, (0, 0, 0, 0)); under.putalpha(text.getchannel('A').point(lambda v: v * 102 // 255))
            img.alpha_composite(under, (dx, dy)); img.alpha_composite(text, (0, 0))
            layered.show(layered.top_hwnd(aw), img, int(ox + W / 2 - img.width / 2), int(oy + hud * ALERT_Y - img.height / 2))
        else:
            ac.delete('all')
            item = ac.create_text(4, 4, text=ALERT_TEXT, fill=ALERT_COLOUR, anchor='nw',
                                  font=(FACE, -max(14, round(hud * ALERT_SIZE)), 'bold'))
            b = ac.bbox(item); w, h = b[2] + 4, b[3] + 4; ac.config(width=w, height=h)
            aw.geometry(f'+{int(ox + W / 2 - w / 2)}+{int(oy + hud * ALERT_Y - h / 2)}')
    def show_alert(f):
        now = time.time()
        screen = meter.last_screen or (0, 0, root.winfo_screenwidth(), root.winfo_screenheight())
        ox, oy, W, H = screen; hud = min(W, H * 16 / 9) * 9 / 16
        if alert['since'] is None: alert['since'] = now
        t = (now - alert['since']) % ALERT_EVERY
        if t < ALERT_SHOW:
            if not alert['shown'] or alert['screen'] != screen: draw_alert_text(ox, oy, W, H, hud)
            alert['shown'] = True
        elif alert['shown']: aw.geometry(OFF); layered.park(layered.top_hwnd(aw)); alert['shown'] = False
        if alert['screen'] != screen:                     # the flash's picture, once per game screen size
            hw = layered.top_hwnd(ew); layered.show(hw, edge_image(int(W), int(H)), int(ox), int(oy))
            layered.click_through(hw); alert['alpha'] = 255
        alert['screen'] = screen
        a = round(255 * hit_opacity(t))
        if a != alert['alpha']: layered.set_alpha(layered.top_hwnd(ew), a); alert['alpha'] = a
    def hide_alert():
        if alert['since'] is None: return
        aw.geometry(OFF); layered.park(layered.top_hwnd(aw)); alert['shown'] = False; alert['since'] = None
        if alert['screen'] is not None: layered.set_alpha(layered.top_hwnd(ew), 0); alert['alpha'] = 0

    # --- "Interlink run sent": a record reached the leaderboards (submit.sent_notice; Sarg's suggestion, 3 Nov), in
    #     Sanctum Helper's notice style - a dark card with a coloured bar, its title over the details - one at a time,
    #     only while the game is in front (they wait otherwise). Where it shows and how it comes in are the player's
    #     (Leaderboards > Sending: leaderboard.notify_place, notify_motion). Three or more waiting at once (a backlog
    #     sent after being offline) make one notice with the count. Sanctum Helper's notices ("Chaos Mode found.") come
    #     here too while the overlay runs (status.json 'notices'): one notice system, one style and spot, never two
    #     cards on top of each other. It drops each as a small JSON file in NOTICES ({head, detail, accent, screen: the
    #     game window's client area, which the meter may not know in a homeland}); read here, then deleted. 'custom':
    #     the player's own spot - unlocked
    #     (notify_locked off), a sample card stays up while the game is in front, to drag there (double-click: back to
    #     the top right); saved as a share of the game's height from its top-left corner, like the DPS window's spot.
    tw = tk.Toplevel(root); tw.overrideredirect(True); tw.attributes('-topmost', True); tw.geometry(OFF)
    toast = {'queue': [], 'since': None, 'alpha': -1, 'sample': None, 'drag': None}
    TOAST_SHOW, TOAST_IN, TOAST_OUT = 4.0, 0.25, 0.35
    TOAST_W, TOAST_H, TOAST_MARGIN = 360, 64, 18
    def toast_font(px, bold):
        for face in (('seguisb.ttf', 'segoeui.ttf') if bold else ('segoeui.ttf',)):
            try: return ImageFont.truetype(face, px)
            except OSError: pass
        return ImageFont.load_default()
    def toast_image(head, detail, accent, s):
        """The card at scale s (1 at 1080p): 360x64, 12 px corners, a 5 px bar in the accent colour, the title in
        white semibold over the details in grey (Sanctum Helper's ToastWindow), each cut short with ... if too long."""
        W, H, r = round(TOAST_W * s), round(TOAST_H * s), round(12 * s)
        img = Image.new('RGBA', (W, H), (0, 0, 0, 0)); d = ImageDraw.Draw(img)
        d.rounded_rectangle((0, 0, W - 1, H - 1), r, fill=(20, 22, 31, 235), outline=(255, 255, 255, 70), width=max(1, round(s)))
        bx, bw = round(14 * s), max(3, round(5 * s))
        d.rounded_rectangle((bx, round(10 * s), bx + bw, H - round(10 * s)), bw // 2 + 1, fill=rgb(accent) + (255,))
        tx = bx + bw + round(12 * s); room = W - tx - round(16 * s)
        def fit(text, font):
            if d.textlength(text, font=font) <= room: return text
            while text and d.textlength(text + '…', font=font) > room: text = text[:-1]
            return text.rstrip(' ·') + '…'
        big, small = toast_font(max(11, round(16 * s)), True), toast_font(max(10, round(14 * s)), False)
        if detail:
            d.text((tx, H * 0.36), fit(head, big), font=big, fill=(255, 255, 255, 255), anchor='lm')
            d.text((tx, H * 0.68), fit(detail, small), font=small, fill=(196, 202, 216, 255), anchor='lm')
        else: d.text((tx, H / 2), fit(head, big), font=big, fill=(255, 255, 255, 255), anchor='lm')
        return img
    def toast_spot(place, ox, oy, W, H, w, h, s):
        """(x, y, the edge it slides from) of a notice of w x h on the game screen (ox, oy, W, H) at scale s."""
        m = round(TOAST_MARGIN * s)
        spot = pos.get('notice')
        if place == 'custom' and isinstance(spot, list) and len(spot) == 2:
            x = min(max(ox, ox + spot[0] * H), ox + W - w); y = min(max(oy, oy + spot[1] * H), oy + H - h)
            return x, y, 'left' if x + w / 2 < ox + W / 2 else 'right'
        if place == 'top-center': return ox + (W - w) / 2, oy + round(150 * s), 'top'     # under a boss's banner
        if place == 'right': return ox + W - w - m, oy + H * 0.42, 'right'
        if place == 'left': return ox + m, oy + H * 0.42, 'left'
        return ox + W - w - m, oy + round((TOAST_MARGIN + 64 + 10) * s), 'right'    # under Sanctum Helper's notices
    def toast_screen():
        screen = meter.last_screen or (0, 0, root.winfo_screenwidth(), root.winfo_screenheight())
        return screen, min(screen[2], screen[3] * 16 / 9) * 9 / 16 / 1080
    def toast_grab(e):
        if toast['sample']: toast['drag'] = (e.x_root - toast['sample'][0], e.y_root - toast['sample'][1])
    def toast_move(e):
        if not toast['drag'] or not toast['sample']: return
        x, y = e.x_root - toast['drag'][0], e.y_root - toast['drag'][1]
        toast['sample'][:2] = [x, y]; present(tw, toast['sample'][2], x, y)
    def toast_drop(e):
        if not toast['drag'] or not toast['sample']: return
        toast['drag'] = None; (ox, oy, W, H), s = toast_screen()
        pos['notice'] = [(toast['sample'][0] - ox) / H, (toast['sample'][1] - oy) / H]; save_positions()
        toast['sample'] = None                                                  # drawn again where it was dropped
    def toast_reset(e):
        pos.pop('notice', None); save_positions(); toast['drag'] = None; toast['sample'] = None
    for ev, fn in (('<Button-1>', toast_grab), ('<B1-Motion>', toast_move), ('<ButtonRelease-1>', toast_drop),
                   ('<Double-Button-1>', toast_reset)):
        tw.bind(ev, fn)
    def toast_sample():
        """Custom and unlocked: the sample card to drag, up while the game is in front and no notice is showing."""
        lb = meter.args.cfg.get('leaderboard') or {}
        want = (lb.get('notify_sent', True) and lb.get('notify_place') == 'custom' and not lb.get('notify_locked', True)
                and toast['since'] is None and not AWAY['on'] and not hide)
        if not want:
            if toast['sample'] and not toast['drag']: park(tw); layered.park(layered.top_hwnd(tw)); toast['sample'] = None
            return
        if toast['sample'] or toast['drag']: return
        (ox, oy, W, H), s = toast_screen()
        img = toast_image('Drag me where record notices should show', 'Lock it in Leaderboards \u203a Sending. Double-click: top right',
                          '#c4cad8', s)
        x, y, _ = toast_spot('custom' if pos.get('notice') else 'top-right', ox, oy, W, H, img.width, img.height, s)
        toast['sample'] = [round(x), round(y), img]; toast['alpha'] = -1
        present(tw, img, round(x), round(y)); layered.click_through(layered.top_hwnd(tw), False)
        layered.set_alpha(layered.top_hwnd(tw), 255)
    def take_notices(now):
        """Sanctum Helper's notice files, four times a second: into the queue in the order they came, then deleted."""
        if now - toast.get('looked', 0) < 0.25: return
        toast['looked'] = now
        try: names = sorted(n for n in os.listdir(NOTICES) if n.endswith('.json'))
        except OSError: return
        for name in names:
            path = os.path.join(NOTICES, name)
            try:
                with open(path, encoding='utf-8') as f: d = json.load(f)
                screen = d.get('screen')
                screen = tuple(int(v) for v in screen) if isinstance(screen, list) and len(screen) == 4 and screen[2] > 0 and screen[3] > 0 else None
                accent = d.get('accent') if re.fullmatch(r'#[0-9a-fA-F]{6}', str(d.get('accent'))) else '#5cb8ff'
                if d.get('head'): toast['queue'].append({'head': str(d['head'])[:120], 'detail': str(d.get('detail') or '')[:120],
                                                         'accent': accent, 'screen': screen, 'kind': str(d.get('from') or 'notice')})
            except (OSError, ValueError, TypeError): pass
            try: os.remove(path)
            except OSError: pass
    def toast_tick():
        now = time.time()
        take_notices(now)
        # (records come as (head, detail, accent) from submit.sent_notice)
        toast['queue'] = [n if isinstance(n, dict) else {'head': n[0], 'detail': n[1], 'accent': n[2], 'screen': None, 'kind': 'record'}
                          for n in toast['queue']]
        toast_sample()
        if toast['sample'] or toast['drag']: return                             # placing it: no notices until locked
        if toast['since'] is None:
            if not toast['queue'] or AWAY['on'] or hide: return
            first = toast['queue'][0]
            records = 0
            while records < len(toast['queue']) and toast['queue'][records]['kind'] == 'record': records += 1
            if records >= 3:                                                   # a backlog of records: one notice
                head, detail, accent = f'{records} records sent to the leaderboards', '', '#7fb2ff'
                del toast['queue'][:records]
            else: head, detail, accent = first['head'], first['detail'], first['accent']; del toast['queue'][0]
            if not meter.last_screen and first.get('screen'):                  # (in a homeland the meter may not know it)
                screen = first['screen']; s = min(screen[2], screen[3] * 16 / 9) * 9 / 16 / 1080
            else: screen, s = toast_screen()
            ox, oy, W, H = screen
            img = toast_image(head, detail, accent, s)
            lb = meter.args.cfg.get('leaderboard') or {}
            x, y, edge = toast_spot(lb.get('notify_place', 'top-right'), ox, oy, W, H, img.width, img.height, s)
            toast.update(since=now, img=img, at=(x, y), edge=edge, motion=lb.get('notify_motion', 'fade'),
                         screen=screen, alpha=-1, shown=None)
            log.info(f"notice: {head}{' - ' + detail if detail else ''}")
        t = now - toast['since']
        if t >= TOAST_SHOW:
            park(tw); layered.park(layered.top_hwnd(tw)); toast['since'] = None; return
        e = min(1.0, t / TOAST_IN, (TOAST_SHOW - t) / TOAST_OUT)
        e = 1 - (1 - e) ** 3                                                   # eases out coming in, in going out
        x, y = toast['at']; img = toast['img']; ox, oy, W, H = toast['screen']
        if toast['motion'] == 'slide':                                        # in from the nearest edge, back out
            gap = {'right': ox + W - x, 'left': x - ox + img.width, 'top': y - oy + img.height}[toast['edge']]
            if toast['edge'] == 'right': x += (1 - e) * gap
            elif toast['edge'] == 'left': x -= (1 - e) * gap
            else: y -= (1 - e) * gap
        elif toast['motion'] == 'drop': y -= (1 - e) * (y - oy + img.height)   # down from the top, back up
        spot = (round(x), round(y))
        if spot != toast['shown'] and not AWAY['on']:
            present(tw, img, *spot); layered.click_through(layered.top_hwnd(tw)); toast['shown'] = spot; toast['alpha'] = -1
        a = round(255 * e)
        if a != toast['alpha'] and not AWAY['on']: layered.set_alpha(layered.top_hwnd(tw), a); toast['alpha'] = a

    ours = set()
    def game_in_front():
        """The game (or one of the overlay's own windows, e.g. while dragging it) is the active window."""
        if not meter.hwnd or dw.dragging or any(L['drag'] for L in hp.values()): return True
        if not ours: ours.update(layered.top_hwnd(w) for w in [dw, aw, ew, tw] + [L['win'] for L in hp.values()])
        if toast['drag']: return True
        fg = ctypes.windll.user32.GetForegroundWindow()
        return fg == meter.hwnd or fg in ours

    def show_fight(d):
        dps_at['bossrush'] = d['bossrush']
        if show_dps_window: show_dps(d['screen'], d['bossrush'], stat_lines(d), d.get('bar'))
        if time.time() - meter.bar_seen < BAR_HIDDEN: show_hp(d); dps_at['hp_up'] = True

    poll_error = {'at': 0.0}
    def poll():
        if not th.is_alive(): root.destroy(); return          # the meter stopped (asked to, or closed): exit
        # an error in one tick is logged (once a minute) and the next tick runs anyway: it used to end the loop with
        # everything frozen as it was ("You're being attacked" stuck over other windows for minutes)
        try: poll_tick()
        except Exception:
            if time.time() - poll_error['at'] > 60: poll_error['at'] = time.time(); log.exception('overlay: error (it keeps going)')
        root.after(15, poll)    # tabbing in/out and new readings show within a frame or two
    drawn = {'at': 0.0, 'pending': None}     # the overlay's last redraw, and the newest reading waiting for its turn
    def poll_tick():
        # the app's picture is of the screen (Desktop Duplication): keep our windows out of it, or the meter would
        # read its own HP labels on the boss bar
        # the HP labels sit on the boss bar, which the meter must read: those stay out of captures. The DPS window stays
        # in screenshots and recordings (unless the player chose otherwise), blacked out of the meter's own pictures
        if time.time() - own.get('published', 0) > 1: publish_own(time.time())
        # Every overlay window shows in screenshots and recordings (unless the player chose otherwise), on any Windows.
        # Where the meter's picture is of the screen (Desktop Duplication) it covers them in its own copy (mask_own),
        # and they step out of captures only while a proof is taken, so a proof is the game alone.
        if hide: hide_all(); meter.own_in_capture = False
        else:
            want_in = not (meter.use_screen and meter.capture_clean())
            if want_in != meter.own_in_capture:
                # in: the meter starts covering them first, then they show in captures; out: they leave captures, then
                # the covering stops (proofs wait SETTLE_S before their first look, many frames after this)
                if want_in: meter.own_in_capture = True
                try:
                    set_in_capture(want_in); meter.own_in_capture = want_in
                except Exception: meter.own_in_capture = True if want_in else None   # unknown: keep covering, try again
        front = game_in_front()
        if AWAY['on'] != (not front):
            AWAY['on'] = not front
            for w, (kind, *where) in list(placed.items()):
                if AWAY['on']: w.geometry(OFF)                         # tabbed out: out of the way
                elif kind == 'img': layered.show(layered.top_hwnd(w), *where)
                else: w.geometry(f'+{int(where[0])}+{int(where[1])}')
        f = meter.fight
        if AWAY['on'] and time.time() - getattr(meter, 'hit_at', 0) < 5:
            show_alert(f)                                               # tabbed out and your HP just dropped
        else: hide_alert()
        latest = None                                          # of several fight frames waiting, only the newest is drawn
        try:
            while True:
                k, d = q.get_nowait()
                if k == 'fight': latest = d; continue
                if k == 'toast': toast['queue'].append(d); continue
                if latest is not None: show_fight(latest); latest = None; drawn['pending'] = None; drawn['at'] = time.time()
                if k == 'fight_end':                                    # the result stays up for a while
                    drawn['pending'] = None                              # a reading still waiting is from before the end
                    hide_hp()
                    keep = 10 if d['bossrush'] else 0 if d.get('holosim') else meter.args.cfg['overlay'].get('result_seconds', 10)
                    if show_dps_window and dps_at['shown'] and keep > 0:
                        show_dps(d['screen'], d['bossrush'], final_lines(d), d.get('bar')); dps_at['hide_at'] = time.time() + keep
                    elif dps_at['shown']: hide_dps()
                elif k == 'status' and d.startswith('fight over'):
                    hide_hp()
                    if dps_at['shown'] and not dps_at['hide_at']: hide_dps()   # no final numbers came (meter closed)
                elif k == 'status' and d.startswith('result recorded'):
                    # result screen: between the player row and the "automatically exit" line
                    show_end(end_note_result(d), 0.665)
                    dps_at['hide_at'] = time.time() + 30
                elif k == 'status' and d.startswith('Holo-Battle Sim recorded'):
                    # Victory screen: between the panel and "press anywhere to close"
                    show_end(end_note_sim(d), 0.875)
                    dps_at['hide_at'] = time.time() + 15
                elif k == 'status' and d.startswith('Battle Data'):
                    # Battle Data: under the player cards
                    show_end(end_note_battle_data(d), 0.765)
                    dps_at['hide_at'] = time.time() + 15
                elif k == 'end_hide':
                    if dps_at.get('end'): hide_dps(); dps_at['end'] = False
        except queue.Empty: pass
        # the numbers redrawn at most 4 times a second (B8: every new reading, up to 60 a second, redrew the window);
        # the newest waits for its turn, and a fight's end shows at once (above)
        if latest is not None: drawn['pending'] = latest
        toast_tick()
        if drawn['pending'] is not None and time.time() - drawn['at'] >= OVERLAY_REDRAW:
            show_fight(drawn['pending']); drawn['pending'] = None; drawn['at'] = time.time()
        if meter.fight and time.time() - meter.bar_seen >= BAR_HIDDEN and dps_at.get('hp_up'): hide_hp(); dps_at['hp_up'] = False
        # Holo-Battle Sim: the numbers too, while its bar is gone (the floor's menus, gizmo picks, the next floor)
        if (meter.fight and meter.holosim_space and dps_at['shown'] and not dps_at['hide_at']
                and time.time() - meter.bar_seen >= 2): hide_dps()
        if dps_at['hide_at'] and time.time() > dps_at['hide_at'] and not meter.fight: hide_dps(); dps_at['hide_at'] = None
    th = threading.Thread(target=meter.run, daemon=True); th.start()
    def backstop():
        # the meter stopped (aLogs closed or turned it off) but the window loop didn't end: never stay behind with
        # an overlay frozen on screen (the alert stayed up after aLogs was closed)
        th.join(); time.sleep(3); log.info('meter stopped but the overlay hung: exiting'); os._exit(0)
    threading.Thread(target=backstop, daemon=True).start()
    poll(); root.mainloop()
    meter.stop = True; th.join(3)

def font_test(out_dir):
    """--font-test <dir>: read the game's HUD font into <dir> and write the outcome to <dir>/result.txt (diagnostics)."""
    import gamefont
    f = gamefont.GameFont.load(out_dir, PLAYER_LOG)
    with open(os.path.join(out_dir, 'result.txt'), 'w', encoding='utf-8') as fh:
        fh.write('ok' if f else f'failed: {gamefont.GameFont.error}\n{gamefont.GameFont.trace or ""}')

def main():
    if len(sys.argv) == 3 and sys.argv[1] == '--font-test':
        os.makedirs(sys.argv[2], exist_ok=True); font_test(sys.argv[2]); return
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--interval', type=float, default=0.05, help='seconds between looks between fights')
    ap.add_argument('--title', default='Aniimo', help='game window title')
    ap.add_argument('--duration', type=int, default=180, help='fight length shown by the countdown (boss rush: 180)')
    ap.add_argument('--level', type=int); ap.add_argument('--boss-id')
    ap.add_argument('--player-log', default=PLAYER_LOG, help='game log to follow for scene changes')
    ap.add_argument('--no-overlay', action='store_true', help='console output only')
    ap.add_argument('--hide-from-capture', action='store_true', help='keep the overlay out of screenshots/recordings')
    ap.add_argument('--no-debug-frames', action='store_true', help="don't save bar crops to logs/<fight>/")
    ap.add_argument('--no-auto-click', action='store_true', help="don't click 'View Battle Data' on the result screen")
    ap.add_argument('--no-submit', action='store_true', help="don't send records to the AniiLogs leaderboards")
    ap.add_argument('--replay', nargs='+', help='feed saved screenshots instead of capturing (for testing)')
    # the packaged meter has no console window: give print() somewhere to go
    if sys.stdout is None: sys.stdout = open(os.devnull, 'w', encoding='utf-8')
    if sys.stderr is None: sys.stderr = open(os.devnull, 'w', encoding='utf-8')
    # settings.json (edited in the desktop app) gives the defaults; command-line flags still win
    cfg = C.load()
    # a blank Game window title means "Aniimo", like the app and Sanctum Helper
    ap.set_defaults(interval=cfg['capture']['look_interval'], title=(cfg['game']['window_title'] or '').strip() or 'Aniimo',
                    player_log=cfg['game']['player_log'] or PLAYER_LOG, no_overlay=not cfg['overlay']['show'],
                    hide_from_capture=not cfg['overlay']['in_recordings'], no_debug_frames=not cfg['capture']['debug_frames'],
                    no_auto_click=not C.automation(cfg, 'auto_open_battle_data'))
    args = ap.parse_args()
    args.cfg = cfg
    setup_log()
    shown = {k: ({kk: vv for kk, vv in v.items() if kk != 'token'} if k == 'leaderboard' else v) for k, v in cfg.items()}
    log.info(f'data folder {C.DATA}; settings {json.dumps(shown)}')     # the Discord sign-in (token) is never logged
    meter = Meter(args, console_out); _meter[0] = meter
    if args.no_overlay or args.replay:
        try: meter.run()
        except KeyboardInterrupt: meter.stop = True
        print()
    else:
        run_overlay(meter, args.hide_from_capture)

if __name__ == '__main__':
    main()
