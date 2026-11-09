"""The meter process's shared core: what every plug-in reading the game uses (the capture, OCR, the game log,
party and character names, settings, the diagnostic log). Moved out of live.py; the plug-ins are dps.py (the DPS
meter) and records.py (the leaderboards), the reading loop is live.py."""
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
# the plug-ins' list, switches and health (plugins.json)
from plugins import PLUGINS, HEALTH, SwitchWatch, guarded, meter_mode, notice_owners, switched_on

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
                        # motion, speed lines; 3.0-3.1 s on 11 Alpha kills, measured frame by frame)
TOP_PART = 0.42         # share of the frame captured while a bar is locked (bar, title, xN, timer are all up there)
INK_MIN = 40            # black pixels a prepared white-text crop needs before it is worth reading (screen_crops)
AWAY_INTERVAL = 0.25    # between fights while the game isn't the active window (a running fight keeps 1/30 s)
HOMELAND_SCENE = 550    # a homeland's scene id ('100-<player id>-550'): nothing is captured there for now
IDLE_INTERVAL = 0.25    # out in the world with no boss bar and nothing to watch: 4 looks a second (a player's lag
                        # report, B4: 8-13 whole 4K window copies a second while just standing at home)
CONFIRM_GAP = 0.09      # a reading that confirms an earlier one (a big drop, a refill, a counter vote) must come at
SETTINGS = os.path.join(HERE, 'overlay_positions.json')

# ---------------------------------------------------------------- diagnostic log
log = logging.getLogger('meter')

class PluginFormatter(logging.Formatter):
    """meter.log's lines, each plug-in's tagged with its name ('[leaderboards] run sent'), so a report shows which part
    said it; the shared core's lines (the 'meter' logger itself) stay as they were."""
    def format(self, record):
        line = super().format(record)
        plugin = record.name.split('.', 1)[1] if record.name.startswith('meter.') else None
        if not plugin: return line
        head = f'{record.levelname:<5} '
        at = line.find(head)
        return line if at < 0 else line[:at + len(head)] + f'[{plugin}] ' + line[at + len(head):]
def setup_log():
    """logs/meter.log: everything the meter does and sees, for bug reports. Capped at 4 x 5 MB."""
    os.makedirs(os.path.dirname(METER_LOG), exist_ok=True)
    h = logging.handlers.RotatingFileHandler(METER_LOG, maxBytes=5 << 20, backupCount=3, encoding='utf-8')
    h.setFormatter(PluginFormatter('%(asctime)s.%(msecs)03d %(levelname)-5s %(message)s', '%Y-%m-%d %H:%M:%S'))
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
BATTLE_DATA_PAUSE = 5               # and holds on Battle Data once it's open, so each is read many times (inputs held)
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

def keep_sample(im, kind, gap=None, kept=None):
    """A few full-size pictures of the screens the readers depend on (the Party screen, an Interlink fight's HUD),
    kept on this PC so a changed screen can be checked against real pictures later: the newest SAMPLES_KEPT of each
    kind, as PNG at the game's own size. Never sent anywhere."""
    now = time.time()
    if now - _sample_at.get(kind, 0) < (SAMPLE_GAP if gap is None else gap): return
    _sample_at[kind] = now
    try:
        d = os.path.join(HERE, 'logs', 'samples'); os.makedirs(d, exist_ok=True)
        # (a short gap is a run of pictures while something's played: JPEGs, which save far quicker than PNGs)
        im.convert('RGB').save(os.path.join(d, f"{kind}_{time.strftime('%Y%m%d_%H%M%S')}." + ('png' if gap is None else 'jpg')), **({} if gap is None else {'quality': 92}))
        for old in sorted(n for n in os.listdir(d) if n.startswith(kind + '_'))[:-(kept or SAMPLES_KEPT)]: os.remove(os.path.join(d, old))
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

# ---------------------------------------------------------------- (from the end of live.py)

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
OVERLAY_REDRAW = 0.25   # s between redraws of the overlay's numbers (4 a second)
BAR_HIDDEN = 0.5   # s without the game's boss bar before the labels around it are hidden too

# everything above, for the plug-ins (from core import *), underscored names and the imported modules too
__all__ = ['AWAY_INTERVAL', 'BAR_GONE_GRACE', 'BAR_GONE_S', 'BAR_HIDDEN', 'BAR_SPOT', 'BATTLE_DATA_PAUSE', 'C', 'CENTER_STARS', 'CHARACTERS', 'CLEAR_STARS', 'CONFIRM_GAP', 'CT', 'CpuLoad', 'E', 'FIGHT_LOG_DAYS', 'HERE', 'HOMELAND_SCENE', 'HP_GRADIENT', 'IDLE_INTERVAL', 'IMAGE_TYPES', 'INK_MIN', 'Image', 'ImageChops', 'ImageDraw', 'ImageFont', 'ImageOps', 'ImageStat', 'InputBlock', 'K', 'KEEP_FOLDERS', 'LEFTOVER_HOURS', 'LogWatcher', 'METER_LOG', 'NOTICES', 'NO_ENGLISH_READS', 'OCR_RECHECK', 'OCR_SAY_AGAIN', 'PluginFormatter', 'OVERLAY_REDRAW', 'OcrWorker', 'PLAYER_LOG', 'Presence', 'R', 'RX_SPACE_UID', 'RX_UID', 'S', 'SAMPLES_KEPT', 'SAMPLE_GAP', 'SETTINGS', 'STAR_CAPS', 'STEADY_ALIKE', 'STEADY_SEEN', 'TEAM', 'TEAMMATES', 'TEST_FRAME_BYTES', 'TEST_FRAME_DAYS', 'TEXT_DIAGNOSTICS', 'TOP_PART', 'Teammates', 'X', '_characters_file', '_last_status', '_meter', '_sample_at', '_status_logged', 'argparse', 'boss_row', 'character_verified', 'clean_old_fight_logs', 'clean_old_screenshots', 'clean_test_frames', 'clear_stars', 'collections', 'console_out', 'csv', 'ctypes', 'difflib', 'fmt', 'hp_colour', 'interlink_score', 'json', 'keep_sample', 'known_characters', 'last_rush_level', 'last_uid', 'load_korean_names', 'load_positions', 'log', 'logging', 'math', 'meter_mode', 'PLUGINS', 'HEALTH', 'SwitchWatch', 'guarded', 'notice_owners', 'switched_on', 'ocr_language_note', 'os', 'party_pill', 'pcstats', 'platform', 'queue', 're', 'refresh_cycles', 'same_name', 'save_picture', 'scene_of', 'screen_changed', 'screen_sig', 'screenshot_cleanup', 'setup_log', 'star_caps', 'stop_handles', 'sys', 'threading', 'time', 'uid_in', 'upload_cycles', 'wc']
