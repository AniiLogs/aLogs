"""Aniimo live boss DPS meter (team damage from the boss HP bar).

Watches the game window with PrintWindow (built-in Windows GDI, no capture border; falls back to a plain screen
copy), reads the boss bar, and shows team damage and DPS in a small always-on-top overlay. It also follows the
game's own Player.log for scene changes, which mark exactly when a boss attempt starts and ends.
It only looks at screen pixels and the game's text log; it never touches the game's memory or files.

Usage:
    python live.py [--interval 0.05] [--no-overlay] [--hide-from-capture] [--no-debug-frames] [--replay shots...]
"""
import argparse, collections, csv, ctypes, difflib, json, logging, logging.handlers, os, platform, queue, re, sys, threading, time
from PIL import Image, ImageOps
import reader as R
import wincapture as wc
import xn_digits as X
from inputblock import InputBlock
import controller as CT
import eggheist as E
import submit as S

import settings as C
HERE = C.DATA                     # everything the meter writes (logs, records, positions)
PLAYER_LOG = os.path.join(os.environ.get('USERPROFILE', ''), 'AppData', 'LocalLow', 'Aniimo', 'Aniimo', 'Player.log')
METER_LOG = os.path.join(HERE, 'logs', 'meter.log')
BAR_GONE_GRACE = 20     # seconds a fight waits for its bar to come back (ultimates and effects can hide it)
KILL_SOUND_WINDOW = (2.5, 6.0)  # the boss kill sound counts when it started this many seconds before the countdown
KILL_TO_COUNTDOWN = 3.0 # seconds from the killing blow to the game's Catch Countdown (the kill-cam: no HUD, slow
                        # motion, speed lines; 3.0-3.1 s on 11 Alpha kills, measured frame by frame)
TOP_PART = 0.42         # share of the frame captured while a bar is locked (bar, title, xN, timer are all up there)
FIGHT_INTERVAL = 1 / 30 # seconds between frames during a fight
INK_MIN = 40            # black pixels a prepared white-text crop needs before it is worth reading (screen_crops)
EGG_BLACK = 0.12        # s of black screen that count as Egg Heist's fade to its end screen (egg_fade)
AWAY_INTERVAL = 0.25    # between fights while the game isn't the active window (a running fight keeps 1/30 s)
CONFIRM_GAP = 0.09      # a reading that confirms an earlier one (a big drop, a refill, a counter vote) must come at
                        # least this long after it, so a one-off effect lasting a few fast frames can't confirm itself
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
RX_UID = re.compile(rb'createVoxelScene new>>>id:\d+-(\d{6,20})-\d+|callerId=(\d{6,20})'
                    rb'|/prod/ClientInfo/(\d{6,20})_client_info_bin|/prod/pet_jewelry/(\d{6,20})')

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
        rush = set(); last = None
        for raw in tail.split(b'\n'):
            m = self.RX_ENTER.search(raw.decode('utf-8', 'replace')) if b'createVoxelScene' in raw else None
            if m: last = m.group(1)
            elif b'fb_bossrush' in raw and last: rush.add(last)
        if last:
            self.current = last
            self.events.put(('enter', last, None))
            if last in rush: self.events.put(('bossrush', last, None))

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
                        if b'createVoxelScene' not in raw and b'clearSpace' not in raw and b'bossrush' not in raw:
                            if b'][info]' in raw and len(self.delays) < 20: self._delay(raw.decode('utf-8', 'replace'))
                            continue
                        line = raw.decode('utf-8', 'replace'); d = self._delay(line)
                        m = self.RX_ENTER.search(line)
                        if m: self.current = m.group(1); self.events.put(('enter', m.group(1), d)); continue
                        m = self.RX_LEAVE.search(line)
                        if m: self.events.put(('leave', m.group(1), d)); continue
                        if 'fb_bossrush' in line: self.events.put(('bossrush', self.current, d))
            except OSError: pass
            time.sleep(0.25)

def scene_of(space_id):
    """'100-<playerId>-<sceneId>' -> sceneId (homeland 550, open world 3000 ...); instances return None."""
    m = re.match(r'^\d+-\d+-(\d+)$', space_id or ''); return int(m.group(1)) if m else None

# Labels exactly as the game writes them (its English text table): a read is snapped to the closest one, so small
# text at 720p ("Tot al Da nn age") still records the game's wording.
RESULT_STATS = ('Total Damage Dealt', 'Total BREAK Damage', 'Targets Defeated', 'Most Healing', 'Most Damage Taken',
                'Most Hits Taken', 'Most Support Provided')     # the Interlink result screen's per-player highlight
EGG_MODES = ('Egg Heist Team Mode', 'Egg Heist')

def canon(text, choices, cutoff=0.6):
    """The game label `text` most likely is (letters only, case-insensitive), or `text` as read if none is close."""
    norm = lambda s: re.sub(r'[^a-z]', '', (s or '').lower())
    t = norm(text)
    if not t: return text
    best = max(choices, key=lambda c: difflib.SequenceMatcher(None, t, norm(c)).ratio())
    return best if difflib.SequenceMatcher(None, t, norm(best)).ratio() >= cutoff else text

CLEAR_STARS = 15                                                 # an Interlink clear: the full rating, then ended early

def first_num(s):
    """First number in an OCR read of tiled short text ('60 60 60' -> 60)."""
    m = re.search(r'\d+', (s or '').replace('O', '0'))
    return int(m.group()) if m else None

def last_uid(path):
    """Your player id from the end of the game log (last 32 MB), the newest of: a personal-space id
    ('100-<uid>-<scene>'), the 'callerId=<uid>' of the game's service calls, or your profile files the game downloads
    right after login. Before login there is none: then there is no UID until the log watcher sees one. Never a
    remembered one - with characters on several servers it could be the wrong character."""
    try:
        with open(path, 'rb') as f:
            f.seek(max(0, os.path.getsize(path) - (32 << 20))); tail = f.read()
    except OSError: return None
    ids = [next(g for g in m.groups() if g) for m in RX_UID.finditer(tail)]
    return ids[-1].decode() if ids else None

def parse_battle_data(text):
    """Battle Data cards: OCR gives each card's 4 labels, then its 4 values in the same order
    (Average DPS, BREAK Value, Damage Taken, Heal Amount). A lone 'o'/'O' is a 0."""
    keys = ('avg_dps', 'break', 'taken', 'heal'); cards = []; vals = []
    for part in (p.strip() for p in (text or '').split(' / ')):
        p = part.replace(',', '')
        if re.fullmatch(r'[oO]', p): p = '0'
        if re.fullmatch(r'\d+', p):
            vals.append(int(p))
            if len(vals) == 4: cards.append(dict(zip(keys, vals))); vals = []
    return cards

# ---------------------------------------------------------------- timer
class Timer:
    """Countdown smoothed against the wall clock: OCR readings are only trusted when they agree with elapsed time.
    It never restarts a fight (combat effects make OCR misread it); fight boundaries come from the game log."""
    def __init__(self, duration): self.duration = duration; self.val = None; self.at = None; self.pending = []
    def predict(self, now): return None if self.val is None else max(0.0, self.val - (now - self.at))
    def feed(self, reading, now):
        """Returns 'ok', 'resync' or None (ignored)."""
        if reading is None or not 0 <= reading <= self.duration: return None
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
        with open(CHARACTERS, 'w', encoding='utf-8') as fh: json.dump(known, fh, ensure_ascii=False)
    except OSError: pass

BAR_SPOT = os.path.join(HERE, 'bar_spot.json')

def screen_sig(im):
    """A 48x27 grey thumbnail of the frame (well under a millisecond), to notice the screen changing."""
    return im.resize((48, 27), Image.NEAREST).convert('L').tobytes()

def screen_changed(a, b, limit=24):
    """Did the screen change (another screen opened or this one closed), not just animate? Mean difference per pixel."""
    return sum(abs(x - y) for x, y in zip(a, b)) / len(a) > limit

IMAGE_TYPES = ('.png', '.jpg', '.jpeg', '.webp', '.bmp')
BATTLE_DATA_WAIT = 45               # seconds a best run waits for Battle Data before it goes with the result screen alone
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
    for d, dirs, names in os.walk(root, topdown=False):
        if os.path.normcase(d).startswith(os.path.normcase(os.path.join(root, 'characters'))): continue
        for n in names:
            if not n.lower().endswith(IMAGE_TYPES): continue
            p = os.path.join(d, n)
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
        except Exception as e:
            log.warning(f'screenshot cleanup: {e!r}')
        for _ in range(3600):
            if meter.stop: return
            time.sleep(1)

def save_picture(im, folder, name):
    """A screen kept as a picture: a JPEG at most 1920 px wide (about 0.3 MB; a 4K PNG is about 5 MB)."""
    d = os.path.join(HERE, 'logs', folder); os.makedirs(d, exist_ok=True)
    if im.width > 1920: im = im.resize((1920, round(im.height * 1920 / im.width)), Image.LANCZOS)
    path = os.path.join(d, name + '.jpg'); im.convert('RGB').save(path, quality=85); return path

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
        try:
            cycles, changed = gamedata.cached_cycles(os.path.join(HERE, 'game_cycles.json'), meter.args.player_log)
            if cycles:
                if changed or not os.path.exists(path):
                    with open(path, 'w', encoding='utf-8') as f: json.dump(cycles, f)
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
                        'bosses': [{'id': str(b), 'name': names.get(str(b), str(b))} for b in c['bossIds']]}
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

class Fight:
    def __init__(self, title, level, row, max_hp, bossrush, debug_dir=None):
        self.title, self.level, self.row, self.max_hp, self.bossrush = title, level, row, max_hp, bossrush
        self.t0 = self.created = time.time(); self.damage = 0; self.samples = collections.deque()
        # Out in the world a boss's bar shows as soon as you're near it, often before anyone attacks: the fight's
        # clock starts at the first hit (between the last reading before the damage and the one that shows it).
        # Interlink's clock runs from the start, like the game's own timer.
        self.hit = bossrush; self.first_damage = None; self.last_at = self.created
        self.low_pending = None      # (fill, time) of a big one-frame drop waiting for a later frame to confirm it
        self.away = 0.0              # seconds of this fight with the game in the background (you tabbed out)
        self.seg = None              # xN of the segment we are in (boss rush)
        self.seg_fill = 1.0          # lowest fill seen in this segment (damage never goes back)
        self.last_fill = None; self.xn_votes = []; self.vote_at = 0.0; self.timer = None; self.elapsed = 0.0; self.avg = self.recent = 0.0
        self.started_at_16 = False; self.refill_pending = 0; self.refill_at = 0.0; self.seg_info = None; self.defeated = False
        self.brk = BreakTracker(R.boss_break_points(row, level))
        name = time.strftime('%Y%m%d_%H%M%S') + '_' + ''.join(c for c in title if c.isalnum())[:24]
        os.makedirs(os.path.join(HERE, 'logs'), exist_ok=True)
        self.log = open(os.path.join(HERE, 'logs', name + '.csv'), 'w', newline='', encoding='utf-8')
        self.w = csv.writer(self.log)
        self.w.writerow(['wall_s', 'timer_s', 'timer_ocr', 'xn_ocr', 'segment', 'fill_pct', 'fill_rgb', 'edge_rgb',
                         'team_damage', 'avg_dps', 'dps_5s', 'note', 'break_left_pct', 'break_phase'])
        self.frames = os.path.join(HERE, 'logs', name) if debug_dir else None
        if self.frames: os.makedirs(self.frames, exist_ok=True)

    def update(self, fill, fill_rgb, edge_rgb, xn_ocr, timer_ocr, cum, duration):
        now = time.time(); note = ''
        if self.timer is None: self.timer = Timer(duration)
        if xn_ocr is not None and not 1 <= xn_ocr <= 16: xn_ocr = None
        # a real reading starts with the live-HP red at the bar's left end; flashes, the ghost covering the whole
        # bar, or the yellow BREAK overlay give other colors - those frames carry no fill information
        r_, g_, b_ = fill_rgb
        if not (r_ >= 225 and 80 <= g_ <= 140 and 75 <= b_ <= 135): fill = None
        # --- segment tracking. Two signals: the counter (digit matcher every frame + OCR) and the bar refilling.
        #     A refill must hold for two frames; two agreeing counter reads can correct the segment either way.
        if self.bossrush:
            if xn_ocr and now - self.vote_at >= CONFIRM_GAP:
                self.xn_votes.append(xn_ocr); self.xn_votes = self.xn_votes[-4:]; self.vote_at = now
            agreed = xn_ocr if xn_ocr and self.xn_votes.count(xn_ocr) >= 2 else None
            if self.seg is None:
                if xn_ocr == 16 or agreed:
                    self.seg = xn_ocr; note = f'segment x{xn_ocr} (counter)'
                    self.started_at_16 = xn_ocr == 16 and fill is not None and fill > 0.97
                    self.seg_fill = fill if fill is not None else 1.0
            elif agreed and agreed != self.seg and abs(agreed - self.seg) <= 3:
                note = f'segment x{self.seg}->x{agreed} (counter)'
                self.seg = agreed; self.seg_fill = fill if fill is not None else 1.0; self.refill_pending = 0
            elif fill is not None and self.seg_fill < 0.25 and fill > 0.7 and fill - self.seg_fill > 0.45:
                if not self.refill_pending or now - self.refill_at >= CONFIRM_GAP:
                    self.refill_pending += 1; self.refill_at = now
                if self.refill_pending >= 2 and self.seg > 1:                            # holds for two readings
                    note = f'segment x{self.seg}->x{self.seg - 1} (bar refilled)'; self.seg -= 1; self.seg_fill = fill
                    self.refill_pending = 0
                    self.xn_votes = [v for v in self.xn_votes if v <= self.seg]
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
        # damage follows the current state, so a corrected segment also corrects the total
        dmg, seg = R.damage_dealt(self.max_hp, self.seg_fill, self.seg if self.bossrush else None, cum, self.bossrush)
        if dmg is not None: self.damage = dmg
        if not self.hit:
            if self.first_damage is None:
                if fill is not None and self.low_pending is None: self.first_damage = self.damage   # from a real reading
            elif self.damage - self.first_damage > self.max_hp * 0.0012:             # a real hit (1 px of the bar is ~0.07%)
                self.hit = True; self.t0 = self.last_at
                note = (note + f' first hit, {self.t0 - self.created:.1f}s after the bar showed up').strip()
        self.last_at = now
        self.seg_info = seg                      # {'grade', 'size_hp', 'left_hp'} of the current star segment, or None
        if note: log.info(f'fight: {note} (fill {self.seg_fill * 100:.1f}%, damage {self.damage:,})')
        self.samples.append((now, self.damage))
        # keep the last sample from before the 5 s window: after a gap with no readings (a BREAK hides the bar for
        # seconds, then all its damage shows at once) the jump is spread over the time it really took, not one frame
        while len(self.samples) > 1 and now - self.samples[1][0] >= 5: self.samples.popleft()
        tp = self.timer.predict(now)
        self.elapsed = (duration - tp) if tp is not None else (now - self.t0 if self.hit else 0.0)
        avg = self.damage / self.elapsed if self.elapsed > 1 else 0
        (t_a, d_a), (t_b, d_b) = self.samples[0], self.samples[-1]
        self.recent = (d_b - d_a) / (t_b - t_a) if t_b - t_a > 0.5 else avg; self.avg = avg
        if not note and now - getattr(self, 'row_at', 0) < CONFIRM_GAP: return None   # ~10 rows a second is plenty
        self.row_at = now
        self.w.writerow([round(now - self.created, 2), None if tp is None else round(tp, 1), timer_ocr, xn_ocr, self.seg,
                         None if fill is None else round(fill * 100, 2), '%02x%02x%02x' % fill_rgb, '%02x%02x%02x' % edge_rgb,
                         self.damage, round(avg), round(self.recent), note,
                         None if self.brk.left is None else round(self.brk.left * 100, 1), self.brk.phase])
        return None

    def mark_defeated(self, why):
        """The boss went down (killed or caught): the bar vanishes before the last hit can be read, so count the rest."""
        if self.defeated: return
        left = self.max_hp - self.damage
        self.defeated = True; self.damage = self.max_hp
        self.avg = self.damage / self.elapsed if self.elapsed > 1 else 0
        try:
            self.w.writerow([round(time.time() - self.created, 2), None, None, None, self.seg, 0.0, '', '', self.damage,
                             round(self.avg), round(self.recent), f'defeated ({why}): final {left:,} HP counted'])
            self.log.flush()
        except ValueError: pass                                         # CSV already closed (confirmed after the fight)

    def close(self):
        try: self.log.close()
        except Exception: pass

class OcrWorker(threading.Thread):
    """Reads text crops on its own OCR helper so the frame loop never waits for OCR (one job at a time)."""
    def __init__(self):
        super().__init__(daemon=True)
        self.ocr = None; self.job = None; self.result = None; self.wake = threading.Event(); self.stop = False
    @property
    def busy(self): return self.job is not None
    def submit(self, crops, meta):
        if self.job is None: self.job = (crops, meta); self.wake.set()
    def take(self):
        r, self.result = self.result, None
        return r
    def run(self):
        try: self.ocr = R.Ocr()                     # its helper starts here, so the meter's first look doesn't wait
        except Exception as e: log.warning(f'background OCR helper did not start: {e!r}'); return
        while not self.stop:
            if not self.wake.wait(0.5): continue
            self.wake.clear()
            if self.stop or self.job is None: continue                  # woken to close
            crops, meta = self.job
            try: text = self.ocr.read(crops)
            except Exception as e:
                log.warning(f'background OCR failed: {e!r}'); text = {}
            self.result = (text, meta); self.job = None
    def close(self):
        self.stop = True; self.wake.set()
        if self.ocr: self.ocr.close()

# ---------------------------------------------------------------- meter
class Meter:
    def __init__(self, args, out):
        self.args, self.out = args, out
        self.catalog, self.cum = R.load_catalog(), R.load_grades(); self.xn_tpl = X.load_templates()
        self.ocr = R.Ocr(); self.fight = None; self.hwnd = None; self.use_screen = False; self.blank = 0
        self.ocr_bg = None if args.replay is not None else OcrWorker()        # the boss bar's title, timer, xN
        self.screen_bg = None if args.replay is not None else OcrWorker()     # the screens' text (check_battle_data)
        for w in (self.ocr_bg, self.screen_bg):
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
        self.duds = []; self.dud_cand = None   # bar-like shapes that never named a boss (dud_check)
        self.hidden_drop = None      # (fill, readings in a row, last counted) of a big drop read at the locked spot
        self.events = queue.Queue(); self.logw = None; self.awaiting = None; self.last_results = 0.0; self.battle_saved = 0.0
        self.last_nobar = 0.0; self.nobar_count = 0; self.last_size = None; self.last_snap = 0.0; self.origin = (0, 0)
        self.after_fight = None      # [fight name, save end screens until, last save]
        self.catch_seen = 0.0        # last time the game's "Catch Countdown" was on screen
        self.stats_seen = 0.0; self.last_scores = {}; self.result_seen = 0.0
        self.party = None            # {'at', 'members': [{'name', 'ai'}]} from the battle prep screen
        self.last_fight = None       # (Fight, ended at) - the result screen doesn't name the boss
        self.last_screen = None      # game area on screen (x, y, w, h), for placing the DPS window
        self.hold = InputBlock(log.info); self.hold_started = 0.0; self.clicks = 0; self.last_click = 0.0
        self.auto_until = 0.0        # automatic "View Battle Data" session running until this time
        self.last_status = 0.0; self.status_text = 'starting'
        self.submitter = None
        self.stop_handles = [] if args.replay else stop_handles()
        if not args.replay and not args.no_submit:
            self.submitter = S.Submitter(); self.submitter.start()
        self.uid = None if args.replay else last_uid(args.player_log)
        # your in-game name, only ever from the game: saved for this UID (the Esc menu's is checked against the UID)
        self.character_name = known_characters().get(self.uid) if self.uid else None
        self.menu_proven = None      # (uid, name) proven from the game menu during this run of the meter
        self.prep_boss = None        # (time, catalog row) of the boss the battle prep screen showed last
        self.rush_level = last_rush_level()   # Interlink bosses' level (the same for every boss of a season)
        if self.uid: log.info(f'player uid {self.uid} (from the game log)')
        if not args.replay and os.path.exists(args.player_log):
            self.logw = LogWatcher(args.player_log, self.events); self.logw.start()

    # -- capture
    def grab(self, partial=False, top_only=False):
        """The game frame. `partial`: only its top part (TOP_PART) - enough to follow a locked boss bar, title, xN
        and timer, and noticeably cheaper at 4K. Coordinates are the same either way; frame_size is the full size.
        `partial` can also be a fraction of the height (the band down to the bar during a fight).
        `top_only`: the top part too, painted into a frame-sized image (the rest black) for the code that expects a
        whole frame; frame_full says whether the frame is whole."""
        part = (TOP_PART if partial is True else partial) if partial else (TOP_PART if top_only else None)
        self.frame_full = not (partial or top_only)
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
        x, y, w, h = wc.client_rect(self.hwnd)
        self.origin = (x, y); self.frame_size = (w, h)       # where the frame sits on screen (for the HP labels)
        if not self.use_screen:
            im = wc.grab_window(self.hwnd, part)
            self.frames += 1
            # a failing PrintWindow gives flat frames; checking costs time, so full frames and every 20th
            if im is None or ((self.frame_full or self.frames % 20 == 0) and wc.is_blank(im)):
                self.blank += 1
                if self.blank >= 8:
                    self.use_screen = True; self.out('status', 'window capture keeps giving no image; using screen capture')
                return None
            self.blank = 0
        else:
            im = wc.grab_screen(x, y, w, int(h * part) if part else h)
        if (w, h) != self.last_size:
            log.info(f'frame size {w}x{h} ({"screen copy" if self.use_screen else "PrintWindow"})')
            self.last_size = (w, h)
        if top_only:
            canvas = getattr(self, 'canvas', None)
            if canvas is None or canvas.size != (w, h): canvas = self.canvas = Image.new('RGB', (w, h))
            canvas.paste(im, (0, 0)); return canvas
        return im

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
        if self.fight:
            f = self.fight
            # The bar vanishes the moment a boss is killed or caught, before the last hit shows - but it also hides
            # during ultimates, so a vanished bar alone proves nothing. Count the rest only with confirmation: the
            # game's Catch Countdown, or the bar staying gone for the whole grace period with the boss nearly down.
            # (Interlink kills are confirmed by the result screen's time, see result_screen.)
            left = (f.max_hp - f.damage) / f.max_hp if f.max_hp else 1
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
                                           'avg': f.damage / f.elapsed if f.elapsed > 1 else 0, 'bossrush': f.bossrush,
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
                if cur <= need: self.exp_card = getattr(self, 'exp_card', []) + [(now, int(n.group(1)), cur, need)]
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

    def time_check(self, f, game_dps):
        """Which fight time the game's Battle Data uses: the time its team DPS implies (damage / DPS) next to the meter's
        two candidates - up to the last frame with the boss bar (what the meter shows now), and up to the fight's end
        (the bar can be hidden by a finishing ultimate). One line per fight in logs/time_check.csv, to settle it."""
        if not game_dps or not f.damage: return
        game_s = f.damage / game_dps; to_end = getattr(f, 'ended', time.time()) - f.t0
        log.info(f"time check: the game's team DPS implies {game_s:.1f}s; the meter counted {f.elapsed:.1f}s "
                 f'(to the last bar reading) and {to_end:.1f}s (to the end of the fight)')
        path = os.path.join(HERE, 'logs', 'time_check.csv'); new = not os.path.exists(path)
        try:
            with open(path, 'a', newline='', encoding='utf-8') as fh:
                w = csv.writer(fh)
                if new: w.writerow(['when', 'boss', 'interlink', 'damage', 'game_team_dps', 'game_seconds', 'meter_seconds', 'to_end_seconds'])
                w.writerow([time.strftime('%Y-%m-%d %H:%M:%S'), f.title, f.bossrush, f.damage, game_dps,
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
                    ('es', 'text', (0.465, 0.73, 0.55, 0.80)))
    QUICK_BOXES = ('h', 'cs', 'lbl', 'cc')   # while a fight's bar is hidden: Catch Countdown and the result screens

    def screen_crops(self, im, quick):
        """The boxes to read, prepared for OCR. A white-text box whose prepared crop has almost no black pixels is
        left out: OCR only ever sees that crop, and a single letter at its size is well over INK_MIN pixels, so
        there's nothing in it to read (reads as '', like before, at no cost)."""
        W, H = im.size; u = min(W, H * 16 / 9); ux = (W - u) / 2
        prep = {'text': R.prep_text, 'bright': R.prep_bright}; out = {}
        for name, kind, (x0, y0, x1, y1) in self.SCREEN_BOXES:
            if quick and name not in self.QUICK_BOXES: continue
            crop = prep[kind](im, (ux + u * x0, H * y0, ux + u * x1, H * y1), u / 2000)
            if kind == 'text' and sum(crop.histogram()[:128]) < INK_MIN: continue
            out[name] = crop
        return out

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
        done = bg.take()
        if done: self.read_screen(done[1][0], done[1][1], done[0])
        if bg.busy or not getattr(self, 'frame_full', True) or not self.screen_read_due(now, quick): return
        self.last_results = now; self.screen_read_sig = getattr(self, "top_sig", None)
        crops = self.screen_crops(im, quick)
        if crops: bg.submit(crops, (im, now))
        else: self.read_screen(im, now, {})                            # nothing on screen to read

    def screen_read_due(self, now, quick=False):
        """Time for the next screen read: 0.1 s after the last one when the screen changed since (seen on the top part
        of the frame, which every look has), about every 1.5 s when it didn't, at once in quick mode."""
        last, cur = getattr(self, 'screen_read_sig', None), getattr(self, 'top_sig', None)
        changed = last is None or cur is None or screen_changed(cur, last, limit=6)
        return now - self.last_results >= (0.0 if quick else 0.1 if changed else 1.5)

    def full_frame_due(self, now):
        """Between fights only the frame's top part is copied (where a boss bar appears; ~3x cheaper at 4K) unless
        the whole frame is needed: for a screen read, the end-screen note, Egg Heist's fade or diagnostic frames."""
        if self.end_sig is not None or getattr(self, 'egg_hold', None) or self.sanctum_in_run(now): return True
        bg = getattr(self, 'screen_bg', None)
        if bg is None or (not bg.busy and self.screen_read_due(now)): return True
        cap = self.args.cfg['capture']; a = self.after_fight
        if a and cap['end_screens'] and now - a[2] >= 3: return True
        return bool(cap['nobar_frames'] and self.bossrush_space and now - self.last_nobar >= 5 and self.nobar_count < 30)

    def read_screen(self, im, now, heads):
        W, H = im.size; u = min(W, H * 16 / 9); ux = (W - u) / 2
        self.game_menu(heads.get('me', ''), heads.get('uid', ''), im, u, ux, H)
        self.profile_notes(heads, im, now, u, ux, H)
        if 'catch' in heads.get('cc', '').lower():                     # a caught boss: its fight was won
            if now - self.catch_seen > 30: log.info(f"Catch Countdown on screen: {heads.get('cc')!r}")
            self.catch_seen = now
        is_result = 'score' in heads.get('lbl', '').lower() and 'time' in heads.get('lbl', '').lower()
        head = heads.get('h', ''); is_bd = 'battle' in head.lower()
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
            self.result_screen(im, now, u, ux, H, heads.get('cs', ''))
            return
        if 'challenge' in heads.get('cs', '').lower(): self.challenge_stats(im, now, u, ux, H); return
        if 'score' in heads.get('bs', '').lower(): self.prep_screen(im, now, u, ux, H, heads['bs']); return
        if 'rank info' in head.lower(): self.egg_rank_info(im, now, u, ux, H); return
        if head.strip().lower().startswith('profile'): self.profile_screen(im, now, u, ux, H); return
        if re.fullmatch(r'\s*\d{1,6}\s*/\s*\d{1,6}\s*', heads.get('ep', '')): self.egg_rank_gain(im, now, u, ux, H, heads['ep'])
        elif re.fullmatch(r'\s*[xX×]\s*\d{1,6}\s*', heads.get('es', '')): self.egg_emperor_count(im, now, u, ux, H, heads['es'])
        if 'adventure' in heads.get('eh', '').lower(): self.egg_result(im, now, u, ux, H, heads['eh']); return
        if not is_bd: return
        self.end_auto('Battle Data is open')
        if self.fight: self.end_fight('Battle Data screen')           # can show up while the bar dropout is still bridged
        # once per appearance: a Battle Data screen left open (or reopened from a menu) is read once, not every 30 s
        shown_already = now - getattr(self, 'bd_seen', 0) < 6; self.bd_seen = now
        if shown_already or now - self.battle_saved < 30: return
        self.battle_saved = now; log.info(f'Battle Data screen detected (header OCR {head!r})')
        # the four stat rows of all cards (dark text on light cards)
        rows = im.crop((int(ux + u * 0.125), int(H * 0.564), int(ux + u * 0.875), int(H * 0.73))).convert('L')
        # smooth (LANCZOS) enlarging keeps the small 720p digits apart, so no card goes missing
        k = max(1.0, 2 * 2000 / u); rows = rows.resize((int(rows.width * k), int(rows.height * k)), Image.LANCZOS)
        text = self.ocr.read({'d': ImageOps.expand(rows, border=20, fill=255)}).get('d', '')
        cards = parse_battle_data(text); vals = [c['avg_dps'] for c in cards]
        log.info(f'Battle Data: OCR {text[:200]!r} -> {cards}')
        lf = self.last_fight if self.last_fight and now - self.last_fight[1] < 180 else None
        stamp = time.strftime('%Y%m%d_%H%M%S'); out_dir = os.path.join(HERE, 'logs', 'battle_data'); os.makedirs(out_dir, exist_ok=True)
        shot = self.picture(im, 'battle_data', stamp, proof=bool(lf and self.run_to_send(lf[0])))
        msg = f'Battle Data screen saved: {os.path.relpath(shot, HERE) if shot else "numbers only (no picture kept)"}'
        if vals: msg += f' - game team DPS {sum(vals):,} (players {", ".join(f"{v:,}" for v in vals)})'
        f = self.awaiting[0] if self.awaiting and now <= self.awaiting[1] else None
        if f and vals:
            game_dps = sum(vals); T = f.elapsed; loss = f.damage / f.max_hp if f.max_hp else 0
            msg += f' vs meter {f.damage / T:,.0f} ({f.title} Lv {f.level})' if T > 1 else ''
            self.time_check(f, game_dps)
            if f.bossrush and f.started_at_16 and loss > 0.02 and T > 60:
                measured = game_dps * T / loss
                predicted = R.boss_max_hp(f.row, f.level, use_calibration=False)
            if f.bossrush and f.started_at_16 and loss > 0.02 and T > 60 and predicted and 0.5 <= measured / predicted <= 2:
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
        screen has stayed black for EGG_BLACK, so a dark flash or a camera cut doesn't block anything."""
        r = self.args.cfg['results']
        hold = getattr(self, 'egg_hold', None)              # [ends at, picture came back at]
        small = im.reduce(max(1, im.width // 64)).convert('L')
        lum = sorted(small.getdata())
        black = lum[len(lum) // 2] < 14 and lum[int(len(lum) * 0.95)] < 40
        if hold:
            if now > hold[0]: self.egg_hold_end('time limit')
            elif black: hold[1] = 0.0
            elif not hold[1]: hold[1] = now
            elif now - hold[1] > 5:
                self.egg_fade_frame(im, 'back'); self.egg_hold_end('the screen came back without an end screen')
            return
        before = getattr(self, 'egg_last_lit', None)
        if not black: self.egg_last_lit = im; self.egg_black = 0; return
        if not getattr(self, 'egg_black', 0): self.egg_black = now                  # when this black stretch began
        if now - self.egg_black < EGG_BLACK or not r.get('egg_fade_hold') or not self.sanctum_in_run(now): return
        if self.hwnd and not wc.is_foreground(self.hwnd): return          # only while you're in the game
        seconds = max(5, min(60, int(r.get('hold_seconds') or 60)))
        self.egg_hold = [now + seconds, 0.0]
        if r.get('hold_inputs', True): self.hold.start(seconds)
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
            with open(path, 'w', encoding='utf-8') as f: json.dump(kept, f)
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
        self.out('status', f"Egg Heist: +{gain} rank points ({st['name']}, {points:,})")
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
        self.stats_seen = now
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
        row = slot_row or next((r for r in self.catalog if r['table'] == 'puppet_data' and r['bossTitle_en'] == boss
                                and int(r['puppetId']) in R.active_cycle_bosses()), None)
        if row: self.prep_boss = (now, row)
        log.info(f'battle prep screen: OCR pill {pill!r}, name {name!r} -> {boss}: best score {score}, stars {stars}')
        if score is not None and boss: self.record_score(im, 'battle_prep', boss, score, stars)

    def cycle_boss(self, text):
        """The boss name on the battle prep screen, matched to one of this week's three bosses. The boss art behind
        the name makes OCR misread it ('Omega eking'), so a read only counts when it clearly matches one; else ''."""
        import difflib
        ids = {str(b) for b in R.active_cycle_bosses()}
        names = sorted({r['bossTitle_en'] for r in self.catalog if r['puppetId'] in ids and r['table'] == 'puppet_data' and r['bossTitle_en']})
        if not names: return text                           # a week the game data doesn't list yet: as read
        def norm(s): return re.sub(r'[^a-z]', '', s.lower())
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
        names = self.ocr.read(crops)
        members = [{'slot': i, 'name': (names.get(f'n{i}') or '').strip(' /|'), 'ai': kind == 'ai', 'self': mine[i]}
                   for i, kind in enumerate(kinds) if kind]
        members = [m for m in members if m['name']]
        if not members: return
        # the same party is read every few seconds: keep each slot's most common reading (OCR slips now and then)
        same = self.party and now - self.party['at'] < 900 and \
            [m['slot'] for m in self.party['members']] == [m['slot'] for m in members]
        votes = self.party['votes'] if same else {}
        for m in members: votes.setdefault(m['slot'], collections.Counter())[m['name']] += 1
        for m in members: m['name'] = votes[m['slot']].most_common(1)[0][0]
        changed = not same or [(m['name'], m['ai'], m['self']) for m in members] != \
            [(m['name'], m['ai'], m['self']) for m in self.party['members']]
        self.party = {'at': now, 'members': members, 'votes': votes}
        if changed:
            log.info('party: ' + ', '.join(f"{m['name']}{' (AI)' if m['ai'] else ''}{' (you)' if m['self'] else ''}" for m in members))

    def party_for_result(self, now):
        """The party read on the prep screen of this run (recent enough to be the same run), or None."""
        return self.party['members'] if self.party and now - self.party['at'] < 900 else None

    def result_screen(self, im, now, u, ux, H, banner):
        """Fight result right after a boss: Challenge Rating (stars), Challenge Score, Completion Time and one
        highlight stat per player. It doesn't name the boss, so it is tied to the fight the meter just tracked.
        Rows go to logs/results.csv; a 'New Best Record' also updates that boss in scores.csv."""
        if now - self.result_seen < 30: return                         # once per appearance
        self.result_seen = now
        if self.fight: self.end_fight('result screen')
        k = u / 2000; crops = {
            'rating': R.prep_short(im, (ux + u * 0.20, H * 0.33, ux + u * 0.275, H * 0.41), k),     # short: tiled
            'score': R.prep_text(im, (ux + u * 0.43, H * 0.325, ux + u * 0.57, H * 0.415), k, size=1),
            'time': R.prep_short(im, (ux + u * 0.68, H * 0.33, ux + u * 0.79, H * 0.41), k)}
        for i in range(4):
            dx = i * 0.1765
            # a softer threshold keeps thin letters at 720p ('Tammy', not 'Ta nny'); just as good at 4K
            crops[f'name{i}'] = R.prep_text(im, (ux + u * (0.15 + dx), H * 0.615, ux + u * (0.22 + dx), H * 0.655), k, thr=160)
            crops[f'stat{i}'] = R.prep_text(im, (ux + u * (0.21 + dx), H * 0.575, ux + u * (0.33 + dx), H * 0.625), k, thr=160)
        t = self.ocr.read(crops)
        num = lambda s: int(re.sub(r'\D', '', s)) if re.search(r'\d', s or '') else None
        rating, score, secs = first_num(t.get('rating')), num(t.get('score')), first_num(t.get('time'))
        players = []
        for i in range(4):
            name = (t.get(f'name{i}') or '').strip(' /|')
            parts = [p.strip() for p in (t.get(f'stat{i}') or '').split(' / ') if p.strip()]
            value = next((num(p) for p in reversed(parts) if re.fullmatch(r'[\d,.\s]+', p)), None)
            label = canon(next((p for p in parts if not re.fullmatch(r'[\d,.\s]+', p)), ''), RESULT_STATS)
            if name or value is not None: players.append({'name': name, 'stat': label, 'value': value})
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
        f = self.last_fight if self.last_fight and now - self.last_fight[1] < 180 else None
        # A clear: finished before the clock ran out with the full 15 stars (the game offers ending early only from 15
        # stars), and the game's time is exact. A run quit early ("The team leader has ended the challenge.") also
        # finishes under the clock, but with fewer stars: that is no clear.
        left = (f[0].max_hp - f[0].damage) / f[0].max_hp if f and f[0].max_hp else 0
        cleared = bool(f and secs is not None and 0 < secs < self.args.duration and f[0].bossrush and (rating or 0) >= CLEAR_STARS)
        if cleared: f[0].elapsed = secs; log.info(f'result screen: cleared in {secs}s')
        elif f and secs is not None and secs < self.args.duration:
            log.info(f'result screen: ended at {secs}s without a clear (rating {rating}, {left:.0%} HP left on the meter)')
        boss = f[0].title if f else '(unknown - meter saw no fight)'
        log.info(f'result screen: banner {banner!r}, OCR {t!r} -> {boss}: rating {rating}, score {score}, time {secs}, players {players}')
        d = os.path.join(HERE, 'logs', 'screens'); os.makedirs(d, exist_ok=True)
        shot = self.picture(im, 'screens', 'result_' + time.strftime('%Y%m%d_%H%M%S'), proof=True)
        path = os.path.join(HERE, 'logs', 'results.csv'); new = not os.path.exists(path)
        with open(path, 'a', newline='', encoding='utf-8') as fh:
            w = csv.writer(fh)
            if new: w.writerow(['time', 'uid', 'cycle', 'boss', 'level', 'new_best', 'rating_stars', 'score', 'time_s',
                                'players', 'meter_damage', 'meter_seconds', 'meter_dps', 'meter_csv', 'screenshot'])
            fi = f[0] if f else None
            w.writerow([time.strftime('%Y-%m-%d %H:%M:%S'), self.uid, R.active_cycle_id(), boss, fi and fi.level,
                        'best' in banner.lower(), rating, score, secs, json.dumps(players, ensure_ascii=False),
                        fi and fi.damage, fi and round(fi.elapsed), fi and round(fi.damage / fi.elapsed) if fi and fi.elapsed > 1 else None,
                        fi and os.path.basename(fi.log.name), os.path.relpath(shot, HERE) if shot else ''])
        party = self.party_for_result(now)
        if party:                                               # AI flags and you come from the prep screen
            for p in players:
                best = max(party, key=lambda m: difflib.SequenceMatcher(None, m['name'].casefold(), p['name'].casefold()).ratio())
                if difflib.SequenceMatcher(None, best['name'].casefold(), p['name'].casefold()).ratio() >= 0.75:
                    p['name'] = best['name']                    # "PuffiMD" on the result screen is the party's "PuffTMD"
                p['ai'] = any(m['ai'] and m['name'] == p['name'] for m in party)
        you = next((m['name'] for m in party or [] if m.get('self')), None)
        for p in players: p['self'] = bool((self.character_name and same_name(p['name'], self.character_name)) or (you and p['name'] == you))
        if f: self.fight_summary(f[0], result={'new_best': 'best' in banner.lower(), 'rating_stars': rating, 'score': score,
                                               'time_s': secs, 'cleared': cleared, 'players': players, 'screenshot': os.path.relpath(shot, HERE) if shot else None},
                                 **({'party': party} if party else {}))
        if 'best' in banner.lower() and score is not None and f: self.record_score(im, 'new_best', boss, score, rating)
        self.out('status', f'result recorded: {boss} - score {score}, {rating} stars, {secs}s')
        self.end_sig = screen_sig(im)                           # the note stays only while this screen does

    # -- automatic "View Battle Data": a short session that clicks the button (and, if enabled in settings, holds
    #    keyboard/mouse and hides the controller) until Battle Data opens, the screen changes, or time runs out.
    def start_auto(self, now):
        r = C.load()['results']
        self.auto_until = now + max(5, min(120, int(r.get('hold_seconds') or 60)))
        self.clicks = 0; self.last_click = 0.0; self.hold_started = now
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
            self.end_auto(f'button reads {label!r}'); return
        x = self.origin[0] + ux + u * 0.6; y = self.origin[1] + H * 0.9
        self.clicks += 1; log.info(f'auto-click {self.clicks}: View Battle Data at screen ({x:.0f}, {y:.0f})')
        self.hide_end_note()
        wc.click_at(x, y)

    def fight_summary(self, f, **parts):
        """logs/fights/<fight>.json: the meter's numbers plus whatever game screens belong to this fight
        (result screen, Battle Data), merged as they appear."""
        d = os.path.join(HERE, 'logs', 'fights'); os.makedirs(d, exist_ok=True)
        path = os.path.join(d, os.path.splitext(os.path.basename(f.log.name))[0] + '.json')
        try:
            with open(path, encoding='utf-8') as fh: s = json.load(fh)
        except (OSError, ValueError):
            s = {'boss': f.title, 'puppetId': f.row['puppetId'], 'level': f.level, 'uid': self.uid, 'cycle': R.active_cycle_id(),
                 'started': time.strftime('%Y-%m-%d %H:%M:%S', time.localtime(f.t0)), 'bossrush': f.bossrush,
                 # the dungeon's own id from the game log: the same for everyone in that run, so two players' apps
                 # sending the run can be told apart from two unrelated runs with the same score
                 'instance': self.space if f.bossrush and self.space and scene_of(self.space) is None else None,
                 'interlink_scene': bool(getattr(f, 'scene_ok', False)),
                 'server': S.game_server(),
                 'meter': {'max_hp': f.max_hp, 'damage': f.damage, 'seconds': round(f.elapsed, 1),
                           'dps': round(f.damage / f.elapsed) if f.elapsed > 1 else None, 'segment_left': f.seg,
                           'tracked_from_start': f.started_at_16, 'csv': os.path.basename(f.log.name),
                           'breaks': f.brk.breaks, 'break_gauge_points': None if not f.brk.size else round(f.brk.size),
                           'break_rate_pct_s': None if f.brk.rate(time.time())[0] is None
                           else round(f.brk.rate(time.time())[0], 2)}}
        s.update(parts)
        with open(path, 'w', encoding='utf-8') as fh: json.dump(s, fh, indent=1, ensure_ascii=False)
        # leaderboard: a run goes, once, when it beats one of your bests on this boss this week (see beats_best),
        # with its result screen and Battle Data. Battle Data's own stats count too, so it decides once that's in.
        if self.submitter and s.get('result') and f.bossrush and 'submit' not in s and not s.get('interlink_scene'):
            log.info("leaderboard: the game log didn't confirm an Interlink scene for this fight - kept in your records, not sent")
            s['submit'] = False
            with open(path, 'w', encoding='utf-8') as fh: json.dump(s, fh, indent=1, ensure_ascii=False)
        if self.submitter and s.get('result') and f.bossrush and 'submit' not in s:
            if s.get('battle_data'): self.decide_run(path)
            else: self.pending_run = (path, time.time() + BATTLE_DATA_WAIT)

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
        card = cards[me] if me is not None and me < len(cards) else {}
        clear = res.get('time_s') if res.get('cleared') else None
        mine = {'score': [res.get('rating_stars') or 0, res.get('score') or 0],
                'clear': -clear if clear is not None else None,             # higher is better for every key
                'dps': card.get('avg_dps'), 'break': card.get('break'), 'taken': card.get('taken'), 'heal': card.get('heal')}
        key = f"{self.uid}|{s.get('cycle')}|{s.get('puppetId')}"
        path = os.path.join(HERE, 'logs', 'bests.json')
        try:
            with open(path, encoding='utf-8') as fh: bests = json.load(fh)
        except (OSError, ValueError): bests = {}
        old = bests.get(key) if isinstance(bests.get(key), dict) else {}
        better = [k for k, v in mine.items() if v not in (None, 0, [0, 0]) and (old.get(k) is None or v > old[k])]
        if better:
            bests[key] = {**old, **{k: mine[k] for k in better}}
            with open(path, 'w', encoding='utf-8') as fh: json.dump(bests, fh)
        return better

    def decide_run(self, path):
        """Battle Data is in (or the wait for it ran out): send the run if it beats one of your bests, else keep it in
        your records only and let its pictures go."""
        try:
            with open(path, encoding='utf-8') as fh: s = json.load(fh)
        except (OSError, ValueError): return
        if 'submit' in s: return
        better = self.beats_best(s)
        s['submit'] = bool(better); s['new_bests'] = better
        with open(path, 'w', encoding='utf-8') as fh: json.dump(s, fh, indent=1, ensure_ascii=False)
        self.pending_run = None
        if better:
            log.info(f"leaderboard: a new best ({', '.join(better)}) - sending this run")
            self.send_run(path)
            return
        log.info('leaderboard: no new best on this boss this week - kept in your records, not sent')
        if int(self.args.cfg['capture'].get('keep_hours', 0)) == 0:   # its proof isn't needed after all
            for shot in ((s.get('result') or {}).get('screenshot'), (s.get('battle_data') or {}).get('screenshot')):
                if shot:
                    try: os.remove(os.path.join(HERE, shot))
                    except OSError: pass

    def send_run(self, path):
        """Send a run once: its numbers, the result screen and Battle Data (whichever pictures exist)."""
        try:
            with open(path, encoding='utf-8') as fh: s = json.load(fh)
        except (OSError, ValueError): return
        if s.get('sent'): return
        s['sent'] = True
        with open(path, 'w', encoding='utf-8') as fh: json.dump(s, fh, indent=1, ensure_ascii=False)
        self.submitter.enqueue(S.run_payload(s, self.uid, self.args.duration), (s.get('result') or {}).get('screenshot'),
                               battle=(s.get('battle_data') or {}).get('screenshot'))

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
                if not row or not self.bossrush_space:
                    log.info(f"leaderboard: prep-screen best for {boss!r} not sent - "
                             + ("not one of this week's bosses" if not row else "the game log didn't confirm an Interlink scene"))
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
                if m: self.set_uid(m.group(1))
                self.space = sid; self.bossrush_space = sid in self.rush_spaces
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
            elif kind == 'uid': self.set_uid(sid)

    def game_menu(self, name, uid_text, im=None, u=0, ux=0, H=0, where='menu'):
        """The menu opened with Esc or Start shows your name with your UID right under it (and the Profile screen
        shows both too, see profile_screen). When that UID is the one from the game log, the name is certainly yours:
        it replaces any earlier guess and is what the leaderboards use. Every Interlink run starts from this menu, so
        a wrong name can't last."""
        digits = re.sub(r'\D', '', (uid_text or '').replace('O', '0'))
        name = re.sub(r'\s+', ' ', (name or '').strip(' /|.'))
        if where == 'menu' and self.uid and self.uid in digits and len(name) <= 2 and im is not None and u:
            name = self.short_name(im, u, ux, H) or name             # "0", "Ab": too short for a plain read
        # the UID under it vouches for the name, so a one-character name counts too ("0" is a real one); just not a
        # lone bit of punctuation, which is OCR noise
        if not self.uid or self.uid not in digits or not 1 <= len(name) <= 40 or (len(name) == 1 and not name.isalnum()):
            # your UID is on screen but no name came out of it: say so (once a minute), so a report shows why
            if self.uid and self.uid in digits and time.time() - getattr(self, 'menu_miss_logged', 0) > 60:
                self.menu_miss_logged = time.time()
                log.info(f'game {where}: UID {self.uid} on screen but its name was not read (OCR {name!r})')
            return
        if self.menu_proven == (self.uid, name): return                  # once per run of the meter
        # the same name twice in a row first: while the menu slides in, one read can be off ("44inMax")
        seen, self.menu_candidate = getattr(self, 'menu_candidate', None), (self.uid, name)
        if seen != (self.uid, name): return
        self.menu_proven = (self.uid, name)
        log.info(f'game {where}: {name} with UID {self.uid} - this is your character')
        self.character_name = name
        remember_character(self.uid, name, verified=True)
        proof = None
        if im is not None and u:
            # the proof the site keeps: the top of the menu - your picture, name and UID - and nothing else
            d = os.path.join(HERE, 'logs', 'characters'); os.makedirs(d, exist_ok=True)
            proof = os.path.join(d, f'{self.uid}.png')
            # (on the Profile screen: its left column, from the name card down to the UID)
            box = (0.6, 0.045, 0.8, 0.14) if where == 'menu' else (0.015, 0.06, 0.28, 0.95)
            im.crop((int(ux + u * box[0]), int(H * box[1]), int(ux + u * box[2]), int(H * box[3]))).save(proof)
        if self.submitter: self.submitter.link_character(self.uid, name, proof=proof)

    def short_name(self, im, u, ux, H):
        """A name of one or two characters (e.g. "0"): Windows OCR drops so short a word, so it's matched against the
        game's own font (namereader). When that isn't sure, OCR reads it after a drawn label ("Player name: 0") at two
        sizes, and a lone round character is told apart by its shape (a zero is narrow, an O round)."""
        box = (ux + u * 0.657, H * 0.068, ux + u * 0.86, H * 0.104); k = u / 2000
        reader = self.name_reader()
        name = reader.read(im, box, u) if reader else None
        if name: return name
        r = self.ocr.read({'a': R.prep_labelled(im, box, k, 'Player name:', size=3),
                           'b': R.prep_labelled(im, box, k, 'Player name:', size=4)})
        a, b = R.after_label(r.get('a'), 'Player name:'), R.after_label(r.get('b'), 'Player name:')
        name = a if a and (a == b or not b) else b if b and not a else a or None
        if name and len(name) == 1 and name in '0Oo':
            aspect = R.ink_aspect(im, box)
            if aspect is not None: name = '0' if aspect < 0.78 else ('o' if name == 'o' else 'O')
        return name

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
        r = self.ocr.read(reads)
        name = re.sub(r'\s+', ' ', r.get('n', '')).strip(' /|.')
        reader = self.name_reader() if mask is not None and len(name) <= 3 else None
        if reader:
            name = reader.read(im, name_box, u, px_4k=namereader.PROFILE_PX_4K, mask=mask) or name
        if not name and mask is not None: log.info(f'profile: name not read ({mask.width}x{mask.height} px of text)')
        self.game_menu(name, r.get('u', ''), im, u, ux, H, where='profile')
        if self.uid and self.uid in re.sub(r'\D', '', r.get('u', '').replace('O', '0')):
            self.profile_stats(im, u, ux, H)

    # Collection Progress on the Profile screen: the value right of each label (dark text on a light pill)
    PROFILE_STATS = {'aniimoPoint': (0.112, 0.512, 0.150, 0.545), 'sparklingAniimo': (0.234, 0.512, 0.259, 0.545),
                     'stylePoints': (0.112, 0.560, 0.150, 0.593), 'prismanaAniimo': (0.234, 0.560, 0.259, 0.593)}

    def profile_stats(self, im, u, ux, H):
        """Your own Profile's Collection Progress (Aniimo Point, Sparkling Aniimo, Style Points, Prismana Aniimo), for
        your AniiLogs profile: white numbers on grey, matched digit by digit against the game's font (OCR, after a
        drawn label, when that isn't sure), kept when two reads in a row agree, and sent when they changed."""
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

    def watch_end_note(self, im):
        """Every frame between fights: hide the end-screen note the moment the screen under it changes (a click,
        Battle Data opening, the screen closing) instead of waiting ~1 s for the next text read to say what's up."""
        if self.end_sig is not None and screen_changed(screen_sig(im), self.end_sig): self.hide_end_note()

    def picture(self, im, folder, name, proof=False):
        """A screen the meter just read. Its data goes into the records (CSV, JSON); the picture is only kept when
        Captures & logs > Keep screenshots says so, or - as leaderboard proof - until it has been sent (the submitter
        deletes it then). None when no picture is kept."""
        keep = int(self.args.cfg['capture'].get('keep_hours', 0))
        sending = proof and self.submitter and self.submitter.config and self.submitter.config['screenshots']
        if keep == 0 and not sending: return None
        return save_picture(im, folder, name)

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
        a.no_debug_frames = not cfg['capture']['debug_frames']; a.no_auto_click = not cfg['results']['auto_open_battle_data']
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

    def step(self):
        self.reload_settings(time.time())
        # out in the world, keep the app listening for the boss kill sound while a fight is on
        if self.fight and not self.fight.bossrush and self.args.cfg.get('audio_cues', {}).get('boss_kill'):
            wc.audio.want(time.time())
        self.game_events()
        self.check_pending_run(time.time())
        # while a fight's bar is locked, the top of the frame is all that's needed (cheaper capture)
        partial = self.fight is not None and self.bar is not None and time.time() - self.bar_seen < 1.0 \
            and self.args.replay is None
        self.band_frame = False
        if partial and not self.text_due(time.time()) and self.frame_size[1]:
            # between text reads only the band from the top down to just under the bar is needed (name, bar, xN,
            # break line): a fraction of the top part, at every resolution
            b = self.bar; band = (b['yc'] + 0.06 * b['span'] + 4) / self.frame_size[1]
            if band < TOP_PART: partial = band; self.band_frame = True
        top_only = (not partial and self.fight is None and self.bar is None and self.args.replay is None
                    and not self.full_frame_due(time.time()))
        im = self.grab(partial, top_only)
        if im is None:
            self.out('status', 'waiting for the game window' if not self.hwnd else "can't see the game window"); return
        now = time.time()
        if self.fight and self.hwnd and self.args.replay is None and not wc.is_foreground(self.hwnd):
            self.fight.away += min(1.0, now - getattr(self, 'last_step', now))   # time the fight ran while you were tabbed out
        self.last_step = now
        self.top_sig = screen_sig(im.crop((0, 0, im.width, max(1, int(self.frame_size[1] * TOP_PART)))))
        if self.frame_full: self.egg_fade(im, now)        # Egg Heist: hold inputs through the fade to its end screen
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
                        with open(BAR_SPOT, 'w', encoding='utf-8') as f: json.dump(self.bar_spots, f)
                    except OSError: pass
            elif self.fight and now - self.bar_seen < BAR_GONE_GRACE:
                # The bar can hide for a while (effects, BREAK, ultimate animations, menus): the fight stays open and
                # continues when the bar comes back. A "Catch Countdown" means the boss was caught.
                self.watch_end_note(im)
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
                        self.fight.update(fill, fill_rgb, edge_rgb, None, None, self.cum, self.args.duration)
                        self.emit_fight(im, self.bar); self.bar_seen = now        # the bar is there, just not findable
                self.snapshot(now); return
            else:
                if self.bar is not None: log.info(f'boss bar lost ({R.LAST_FAIL[0]})')
                self.bar = None
                if self.fight: self.end_fight('boss bar gone')
                self.watch_end_note(im)
                self.check_battle_data(im, now)
                self.save_end_screens(im, now)
                self.save_nobar(im, now)
                self.out('status', 'waiting for a boss bar')   # why it isn't found goes to meter.log (1 s snapshot)
                self.snapshot(now); return
        bar = self.bar; self.bar_seen = now
        fill, fill_rgb, edge_rgb = R.measure_fill_any(im, bar)
        scale = bar['span'] / R.REF_SPAN
        need_title = self.fight is None or now - self.last_title > 8
        timer = None; text = None
        xn_box = R.box_at(bar, R.XN_BOX)
        xn = xn_digits = X.read_counter(im.crop(tuple(int(v) for v in xn_box)), self.xn_tpl)   # digit shapes, every frame
        if not self.fight: self.instant_start(xn, now)
        if self.dud_check(bar, fill, xn, now):
            self.bar = None; self.out('status', 'waiting for a boss bar'); self.snapshot(now); return
        # Text (timer, title, xN fallback) is read about once a second. Live, a background OCR thread does it so a
        # frame never waits for it; the result is used on the frame after it arrives. Replays read it in place.
        done = self.ocr_bg.take() if self.ocr_bg else None
        if done:
            text, meta = done
            timer = R.parse_timer(text.get('timer'))
            if meta['xn_missing'] and xn is None: xn = R.parse_xn(text.get('xn'))
            if meta['title']: self.identify(text.get('title'), xn)
        if self.text_due(now) and not self.band_frame:
            crops = {'timer': R.prep_text(im, R.timer_box(*self.frame_size), scale)}
            if xn is None: crops['xn'] = R.prep_short(im, xn_box, scale)            # OCR as a second opinion
            if need_title: crops['title'] = R.prep_text(im, R.box_at(bar, R.TITLE_BOX), scale)
            self.last_text = now
            if need_title: self.last_title = now
            meta = {'xn_missing': xn is None, 'title': need_title}
            if self.ocr_bg: self.ocr_bg.submit(crops, meta)
            else:
                text = self.ocr.read(crops)
                timer = R.parse_timer(text.get('timer'))
                if xn is None: xn = R.parse_xn(text.get('xn'))
                if need_title: self.identify(text.get('title'), xn)
        if self.fight:
            self.save_debug(im, bar, now)
            if now - getattr(self, 'brk_at', 0) >= CONFIRM_GAP:   # ~10 readings a second: a refill shows as a jump between two
                self.brk_at = now                                   # readings, sized for that pace
                gauge, kind, _ = R.measure_break(im, bar)
                self.fight.brk.feed(gauge, kind, now)
            self.fight.update(fill, fill_rgb, edge_rgb, xn, timer, self.cum, self.args.duration)
        snap = dict(bar=bar, fill=fill, fill_rgb=fill_rgb, edge_rgb=edge_rgb, xn_digits=xn_digits, xn_ocr=xn)
        if text: snap['ocr'] = text
        self.snapshot(now, **snap)
        if self.fight: self.emit_fight(im, bar)

    def text_due(self, now):
        """Time to read the bar's text (timer, name, xN fallback): about once a second, the name every 8 s."""
        need_title = self.fight is None or now - self.last_title > 8
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
        self.last_screen = (self.origin[0], self.origin[1], self.frame_size[0], self.frame_size[1])
        self.out('fight', {'title': f.title, 'level': f.level, 'max_hp': f.max_hp, 'xn': f.seg, 'fill': f.seg_fill,
                           'damage': f.damage, 'elapsed': f.elapsed, 'avg': f.avg, 'recent': f.recent,
                           'bossrush': f.bossrush, 'seg_size': si.get('size_hp'), 'seg_left': si.get('left_hp'),
                           'bar': (self.origin[0] + bar['left_bracket'], self.origin[1] + bar['yc'], bar['span']),
                           'break': f.brk.info(time.time()), 'screen': self.last_screen})

    def identify(self, title_text, xn):
        """Match the boss. A running fight keeps its boss unless 3 clean reads in a row say otherwise."""
        level, title = R.parse_title(title_text)
        level = self.args.level or level
        in_world = self.space is not None and scene_of(self.space) is not None   # open world / homeland, per the game log
        bossrush = not in_world and (xn is not None or self.bossrush_space or (self.fight is not None and self.fight.bossrush))
        row, _ = R.pick_boss(self.catalog, title, bossrush, self.args.boss_id) if title and level else (None, 0)
        if not row:
            if not self.fight and title and self.bar is not None:
                self.out('status', f'reading boss name... ("{title}" Lv {level})')
            return
        if self.fight:
            if row['puppetId'] == self.fight.row['puppetId'] and level == self.fight.level:
                self.switch_votes = []; return
            self.switch_votes.append((row['puppetId'], level)); self.switch_votes = self.switch_votes[-3:]
            if len(self.switch_votes) < 3 or len(set(self.switch_votes)) != 1: return
            self.end_fight('different boss')
        self.start_fight(row, level, title, bossrush, f'title read {title_text!r}, xN {xn}')
        if bossrush: self.rush_level = level

    def start_fight(self, row, level, title, bossrush, how):
        hp = R.boss_max_hp(row, level)
        self.fight = Fight(row['bossTitle_en'] or title, level, row, hp, bossrush, debug_dir=not self.args.no_debug_frames)
        self.fight.scene_ok = bossrush and self.bossrush_space    # the game log confirmed an Interlink scene
        if self.bar: self.fight.bar_geom = (self.bar['span'], self.bar['yc'])     # the bar this fight follows
        self.out('status', f'fight started: {self.fight.title} Lv {level}, max HP {hp:,}' + (' (Interlink)' if bossrush else ''))
        log.info(f"fight details: puppetId {row['puppetId']}, {how}, "
                 f"bossrush space {self.bossrush_space}, csv {os.path.basename(self.fight.log.name)}")

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

    def write_status(self, now):
        """status.json for the desktop app's dashboard, about once a second."""
        if now - self.last_status < 1.0: return
        self.last_status = now; f = self.fight; sub = self.submitter
        C.write_status({
            'time': now, 'pid': os.getpid(), 'status': self.status_text,
            'game_window': bool(self.hwnd), 'capture': 'screen copy' if self.use_screen else 'PrintWindow',
            'frame': list(self.last_size) if self.last_size else None, 'boss_bar': self.bar is not None,
            'fight': f and {'boss': f.title, 'level': f.level, 'segment': f.seg, 'damage': f.damage, 'max_hp': f.max_hp,
                            'seconds': round(f.elapsed), 'avg_dps': round(f.avg), 'recent_dps': round(f.recent)},
            'character': {'uid': self.uid, 'name': self.character_name, 'verified': character_verified(self.uid),
                          'dispute': self.submitter.dispute if self.submitter else None},
            'auto_battle_data': bool(self.auto_until),
            'leaderboard': {'on': bool(sub and sub.config), 'pending': sub.pending() if sub else 0,
                            'last': sub.last_result if sub else None},
        })

    def run(self):
        if self.args.replay is None:                     # this week's bosses from the game data, after each patch
            threading.Thread(target=refresh_cycles, args=(self,), daemon=True, name='game-cycles').start()
            threading.Thread(target=screenshot_cleanup, args=(self,), daemon=True, name='screenshot-cleanup').start()
        while not self.stop:
            t = time.time()
            if self.asked_to_stop(): break
            try: self.step()
            except Exception as e:
                log.exception('error in step'); self.out('status', f'error: {e!r} (details in logs/meter.log)')
            self.write_status(time.time())
            if self.args.replay is not None and not self.args.replay: break
            interval = FIGHT_INTERVAL if self.fight else self.args.interval
            if not self.fight and self.hwnd and self.args.replay is None and not wc.is_foreground(self.hwnd):
                interval = max(interval, AWAY_INTERVAL)     # tabbed out between fights: a lighter pace
            time.sleep(max(0, interval - (time.time() - t)))
        self.end_auto('meter closed'); self.hold.stop(); self.end_fight('meter closed'); self.ocr.close()
        if self.ocr_bg: self.ocr_bg.close()
        if self.screen_bg: self.screen_bg.close()
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

_last_status = [None]
_meter = [None]
def console_out(kind, data):
    if kind == 'status':
        if _meter[0]: _meter[0].status_text = data
        if data != _last_status[0]:
            print('\n' + time.strftime('%H:%M:%S'), data, flush=True); log.info(data)
        _last_status[0] = data
    elif kind == 'fight':                             # the overlay's other messages (fight_end, end_hide) aren't printed
        s = f"x{data['xn']}" if data['xn'] else ''
        print(f"\r{data['title']} Lv{data['level']} {s} fill {data['fill'] * 100:5.1f}% | dmg {fmt(data['damage'])} | "
              f"avg {fmt(data['avg'])}/s | 5s {fmt(data['recent'])}/s | {data['elapsed']:.0f}s   ", end='', flush=True)

def load_positions():
    try:
        with open(SETTINGS, encoding='utf-8') as f: return json.load(f)
    except (OSError, ValueError): return {}

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
    pos = load_positions()
    root = tk.Tk(); root.withdraw()
    def save_positions():
        try:
            with open(SETTINGS, 'w', encoding='utf-8') as f: json.dump(pos, f)
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
    def present(win, image, x, y):
        """With the game font: the window is this RGBA image, with real per-pixel alpha (layered.py)."""
        placed[win] = ('img', image, x, y)
        if not AWAY['on']: layered.show(layered.top_hwnd(win), image, x, y)
    def place(win, x, y):
        """Without the game font: move the window there (a plain Tk window)."""
        placed[win] = ('geo', x, y)
        if not AWAY['on']: win.geometry(f'+{int(x)}+{int(y)}')
    def park(win): placed.pop(win, None); win.geometry(OFF)

    def compose(lines, H, divider=False, center=False, backdrop=None):
        """The stats as one RGBA image in the game font: lines [(text, colour, size, 'i'/'b')], optional divider.
        backdrop: the dark card's strength in % (the Overlay setting unless given)."""
        backdrop = BACKDROP if backdrop is None else backdrop
        f = GF['font']; pad = max(3, round(H * (0.011 if backdrop else 0.004))); parts = []
        for text, colour, size, *style in lines:
            px = max(11, round(H * size)); style = style[0] if style else ''
            parts.append((f.render(text, px, rgb(colour), italic='i' in style, bold='b' in style), px))
        width = max([im.width for im, _ in parts] + ([round(H * 0.215)] if divider else []))
        lw = max(1, round(H / 1080)); top = round(H * 0.01) if divider else 0
        height = top + sum(im.height + round(px * 0.04) for im, px in parts)
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
            out.alpha_composite(im, (pad + ((width - im.width) // 2 if center else 0), y))
            y += im.height + round(px * 0.04)
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
    def dps_start(e): drag['x'], drag['y'] = e.x_root - dw.winfo_x(), e.y_root - dw.winfo_y(); dw.dragging = True
    def dps_move(e): dw.geometry(f'+{e.x_root - drag["x"]}+{e.y_root - drag["y"]}')
    def dps_drop(e=None):
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
        for text, colour, size, *style in lines:
            px = max(11, round(H * size)); style = style[0] if style else ''
            item = styled(dc, pad, y, text, px, colour, italic='i' in style, bold='b' in style)
            y = dc.bbox(item)[3] + round(px * 0.12)
        box = dc.bbox('all')
        if center:                                    # end screens: each line centred on the widest
            for item in dc.find_all():
                b = dc.bbox(item); dc.move(item, (box[2] - b[2]) // 2, 0)
        if BACKDROP:                                  # without the game font: a solid panel (no partial see-through here)
            dc.tag_lower(dc.create_rectangle(0, 0, box[2] + pad, box[3] + pad, fill=BG, outline=''))
        dc.config(width=box[2] + pad, height=box[3] + pad)
    def show_dps(screen, bossrush, lines=None):
        # the game's HUD is a 16:9 area (centred on ultrawide): spots and text size follow its height, which is the
        # screen's on 16:9 and ultrawide, and the width's 9/16 on 16:10, where the HUD keeps to the top
        ox, oy, W, H = screen; u = min(W, H * 16 / 9); ux = ox + (W - u) / 2; H = u * 9 / 16
        if bossrush and meter.args.cfg['overlay'].get('rush_position') == 'panel':
            base, key = (ux + u * 0.024, oy + H * 0.335), 'dps_offset_rush'              # under the stars/timer panel
        elif bossrush: base, key = (ux + u * 0.068, oy + H * 0.022), 'dps_offset_rush_top'  # top left, right of Esc
        elif meter.args.cfg['overlay'].get('world_position') == 'quests':
            base, key = (ux + u * 0.024, oy + H * 0.415), 'dps_offset_world'             # under the quest list
        else: base, key = (ux + u * 0.168, oy + H * 0.09), 'dps_offset_map'              # right of the minimap
        dps_at['base'], dps_at['h'], dps_at['key'] = base, H, key
        if lines is not None and lines != dps_at['lines']:
            dps_at['lines'] = lines; render_stats(lines, H)
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
        dps_at['lines'] = lines; render_stats(lines, H)
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

    # --- HP labels: plain white text with a soft dark shadow, like the game's own bar text, attached to the boss bar
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

    def draw(name, text, bar, colour='#ffffff'):
        """Text at its spot next to the bar. bar = (left bracket x, bar centre y, bracket span) on screen."""
        L = hp[name]; c = L['c']; fx, fy, anchor, fs = HP_SPOTS[name]; bx, by, span = bar
        px = max(11, round(span * fs)); pad = max(2, round(px / 13)) + 3
        if GF['font']:
            img = GF['font'].render(text, px, rgb(colour), bold=name == 'pct')
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
    if hide:   # only on request: by default the meter shows up in screenshots and recordings
        for w in [dw] + [L['win'] for L in hp.values()]:
            try: wc.hide_from_capture(ctypes.windll.user32.GetParent(w.winfo_id()) or w.winfo_id())
            except Exception: pass

    def show_hp(d):
        bar = d.get('bar')
        if not bar or not show_hp_labels: hide_hp(); return
        left = max(0, d['max_hp'] - d['damage']); max_hp = d['max_hp'] or 1
        draw('pct', f'{left / max_hp * 100:.1f}%', bar)
        if d['bossrush'] and d['xn'] and d['seg_size']: draw('bar', f"{fmt(d['seg_left'])} / {fmt(d['seg_size'])}", bar)
        else: clear('bar')
        draw('total', f"HP {fmt(left)} / {fmt(max_hp)}", bar)
        brk = break_text(d.get('break') or {})
        if brk: draw('break', brk[0], bar, brk[1])
        else: clear('break')

    def stat_lines(d):
        """The stats: DPS, the last 5 s and team damage."""
        return [(f"{fmt(d['avg'])} DPS", '#ffd36a', 0.0235, 'ib'),         # big italic number, like the HUD's
                (f"Last 5 s {fmt(d['recent'])} DPS · {d['elapsed']:.0f} s", '#ffffff', 0.015),
                (f"Team damage {fmt(d['damage'])} / {fmt(d['max_hp'])}", '#ffffff', 0.015)]

    def final_lines(d):
        """The fight's result, left in the stats spot for a few seconds: the final hit is counted by then."""
        return [(f"{fmt(d['avg'])} DPS", '#ffd36a', 0.0235, 'ib'),
                (('Defeated' if d['defeated'] else 'Fight over') + f" · {d['elapsed']:.0f} s", '#ffffff', 0.015),
                (f"Team damage {fmt(d['damage'])} / {fmt(d['max_hp'])}", '#ffffff', 0.015)]

    def break_text(b):
        """(text, colour) for the break gauge line under the boss bar, or None while it can't be read."""
        if b.get('left') is not None or b.get('breaks'):
            rate = b.get('rate_now') if b.get('rate_now') is not None else b.get('rate')
            count = f" · {b['breaks']} break{'s' if b['breaks'] != 1 else ''}" if b.get('breaks') else ''
            if b.get('phase') == 'broken': text, colour = f"BROKEN{count}", '#ffe066'
            elif b.get('phase') == 'refill': text, colour = f"Break gauge refilling{count}", '#c8d0dc'
            else:
                size = b.get('size')
                if size:      # break points from the game data: what's left and break points per second
                    text = (f"Break {fmt(b['left'] * size)} / {fmt(size)}"
                            + (f" · {fmt(rate * size / 100)}/s" if rate is not None else '') + count)
                else:
                    text = f"Break {b['left'] * 100:.0f}% left" + (f" · {rate:.1f}%/s" if rate is not None else '') + count
                colour = '#ffc58a' if b.get('kind') == 'orange' else '#dfe8f5'
            return text, colour
        return None

    # --- "You're being attacked": while the game isn't in front and a boss fight is running (your party fights on in
    #     the background), shown above every window the way the game shows it in its menus (notice 50003, "Under
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
        elif alert['shown']: aw.geometry(OFF); alert['shown'] = False
        if alert['screen'] != screen:                     # the flash's picture, once per game screen size
            hw = layered.top_hwnd(ew); layered.show(hw, edge_image(int(W), int(H)), int(ox), int(oy))
            layered.click_through(hw); alert['alpha'] = 255
        alert['screen'] = screen
        a = round(255 * hit_opacity(t))
        if a != alert['alpha']: layered.set_alpha(layered.top_hwnd(ew), a); alert['alpha'] = a
    def hide_alert():
        if alert['since'] is None: return
        aw.geometry(OFF); alert['shown'] = False; alert['since'] = None
        if alert['screen'] is not None: layered.set_alpha(layered.top_hwnd(ew), 0); alert['alpha'] = 0

    ours = set()
    def game_in_front():
        """The game (or one of the overlay's own windows, e.g. while dragging it) is the active window."""
        if not meter.hwnd or dw.dragging or any(L['drag'] for L in hp.values()): return True
        if not ours: ours.update(layered.top_hwnd(w) for w in [dw, aw, ew] + [L['win'] for L in hp.values()])
        fg = ctypes.windll.user32.GetForegroundWindow()
        return fg == meter.hwnd or fg in ours

    def show_fight(d):
        dps_at['bossrush'] = d['bossrush']
        if show_dps_window: show_dps(d['screen'], d['bossrush'], stat_lines(d))
        if time.time() - meter.bar_seen < BAR_HIDDEN: show_hp(d); dps_at['hp_up'] = True

    def poll():
        if not th.is_alive(): root.destroy(); return          # the meter stopped (asked to, or closed): exit
        front = game_in_front()
        if AWAY['on'] != (not front):
            AWAY['on'] = not front
            for w, (kind, *where) in list(placed.items()):
                if AWAY['on']: w.geometry(OFF)                         # tabbed out: out of the way
                elif kind == 'img': layered.show(layered.top_hwnd(w), *where)
                else: w.geometry(f'+{int(where[0])}+{int(where[1])}')
        f = meter.fight
        if AWAY['on'] and f and time.time() - meter.bar_seen < 3: show_alert(f)    # tabbed out, boss bar still up
        else: hide_alert()
        latest = None                                          # of several fight frames waiting, only the newest is drawn
        try:
            while True:
                k, d = q.get_nowait()
                if k == 'fight': latest = d; continue
                if latest is not None: show_fight(latest); latest = None
                if k == 'fight_end':                                    # the result stays up for a while
                    hide_hp()
                    keep = 10 if d['bossrush'] else meter.args.cfg['overlay'].get('result_seconds', 10)
                    if show_dps_window and dps_at['shown'] and keep > 0:
                        show_dps(d['screen'], d['bossrush'], final_lines(d)); dps_at['hide_at'] = time.time() + keep
                    elif dps_at['shown']: hide_dps()
                elif k == 'status' and d.startswith('fight over'):
                    hide_hp()
                    if dps_at['shown'] and not dps_at['hide_at']: hide_dps()   # no final numbers came (meter closed)
                elif k == 'status' and d.startswith('result recorded'):
                    # result screen: between the player row and the "automatically exit" line
                    show_end(end_note_result(d), 0.665)
                    dps_at['hide_at'] = time.time() + 30
                elif k == 'status' and d.startswith('Battle Data'):
                    # Battle Data: under the player cards
                    show_end(end_note_battle_data(d), 0.765)
                    dps_at['hide_at'] = time.time() + 15
                elif k == 'end_hide':
                    if dps_at.get('end'): hide_dps(); dps_at['end'] = False
        except queue.Empty: pass
        if latest is not None: show_fight(latest)
        if meter.fight and time.time() - meter.bar_seen >= BAR_HIDDEN and dps_at.get('hp_up'): hide_hp(); dps_at['hp_up'] = False
        if dps_at['hide_at'] and time.time() > dps_at['hide_at'] and not meter.fight: hide_dps(); dps_at['hide_at'] = None
        root.after(15, poll)    # tabbing in/out and new readings show within a frame or two
    th = threading.Thread(target=meter.run, daemon=True); th.start(); poll(); root.mainloop()
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
    ap.set_defaults(interval=cfg['capture']['look_interval'], title=cfg['game']['window_title'],
                    player_log=cfg['game']['player_log'] or PLAYER_LOG, no_overlay=not cfg['overlay']['show'],
                    hide_from_capture=not cfg['overlay']['in_recordings'], no_debug_frames=not cfg['capture']['debug_frames'],
                    no_auto_click=not cfg['results']['auto_open_battle_data'])
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
