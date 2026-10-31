"""Sends the meter's records to the AniiLogs leaderboards - only when the player opts in.

Configured by the "leaderboard" section of settings.json (edited in the desktop app): submit on/off, the API URL,
and a desktop token created on the AniiLogs leaderboard page while signed in with Discord.

Every record goes to logs/submit_queue/ first and is sent from there by a background thread, so nothing is lost
while offline; a record that comes back with more data (e.g. Battle Data after the result screen) replaces the
queued one and updates the same leaderboard entry (same run id).
"""
import io, json, logging, os, re, threading, time, urllib.error, urllib.request, uuid
from PIL import Image, ImageOps

import settings as C
HERE = C.DATA
QUEUE = os.path.join(HERE, 'logs', 'submit_queue')
CLIENT_VERSION = os.environ.get('ALOGS_VERSION') or 'meter-0.1'   # e.g. aLogs-0.1.28+rc.8, from the app
log = logging.getLogger('meter')

def load_config():
    """The leaderboard settings, or None while submissions are off or no token is set."""
    c = C.load()['leaderboard']
    if not c.get('submit') or not c.get('api_url') or not str(c.get('token', '')).startswith('alb_'): return None
    return {'api_url': c['api_url'].rstrip('/'), 'token': c['token'], 'screenshots': bool(c.get('include_screenshots', True)),
            'link_character': bool(c.get('link_character', True))}

def write_json(path, data, **kw):
    """Written whole or not at all (a crash mid-write left a truncated file that stopped the whole queue)."""
    tmp = path + '.tmp'
    with open(tmp, 'w', encoding='utf-8') as f: json.dump(data, f, **kw)
    os.replace(tmp, path)

def _safe(name):
    return ''.join(ch if ch.isalnum() or ch in '-_.' else '_' for ch in name)[:120]

PROOF_SIZE = (1280, 720)     # every proof the same small size, whatever the screen (~75-130 KB; the site takes <=1.5 MB)

def _evidence_bytes(path):
    """The run's proof screenshot as a 1280x720 JPEG (a 16:9 screen; other shapes are fitted inside it)."""
    im = Image.open(path).convert('RGB')
    im = ImageOps.pad(im, PROOF_SIZE, Image.LANCZOS, color=(0, 0, 0)) if im.size != PROOF_SIZE else im
    buf = io.BytesIO(); im.save(buf, 'JPEG', quality=85, optimize=True); return buf.getvalue()

# Two proofs for each screen a run is read from: the part aLogs read, cropped at the screen's own sharpness ('evidence',
# 'battle': what the site shows), and the whole screen ('evidence_full', 'battle_full', at most 1920 wide) for the site's
# second check, which reads the numbers again on its own. Boxes are shares of the 16:9 UI area (x0, y0, x1, y1) and go
# with the run (extra.proofBoxes), so the site knows where in the whole screen each crop came from.
PROOF_BOXES = {'interlink_run': {'evidence': (0.13, 0.06, 0.88, 0.67),     # banner, rating, score, time, the players' names and stats
                                 'battle': (0.115, 0.295, 0.885, 0.755)}}  # Battle Data: the four cards, names and all four stats
FULL_WIDTH = 1920

def _jpeg(im, quality):
    buf = io.BytesIO(); im.save(buf, 'JPEG', quality=quality, optimize=True); return buf.getvalue()

def _crop_bytes(path, box):
    """The part of a proof screenshot inside `box` (shares of its 16:9 UI area), at full size."""
    im = Image.open(path).convert('RGB'); W, H = im.size; u = min(W, H * 16 / 9); ux = (W - u) / 2
    return _jpeg(im.crop((int(ux + u * box[0]), int(H * box[1]), int(ux + u * box[2]), int(H * box[3]))), 90)

def _full_bytes(path):
    """A whole proof screenshot for the site's second check, at most FULL_WIDTH wide."""
    im = Image.open(path).convert('RGB')
    if im.width > FULL_WIDTH: im = im.resize((FULL_WIDTH, round(im.height * FULL_WIDTH / im.width)), Image.LANCZOS)
    return _jpeg(im, 88)

PARTY_MAX = 1_500_000      # bytes: the site takes the Party screen up to 1.5 MB

def _party_bytes(path):
    """The run's Party screen (its last capture before the fight), whole, as a JPEG under PARTY_MAX."""
    im = Image.open(path).convert('RGB')
    for width, quality in ((2560, 85), (2560, 75), (1920, 80), (1600, 70)):
        small = im.resize((width, round(im.height * width / im.width)), Image.LANCZOS) if im.width > width else im
        data = _jpeg(small, quality)
        if len(data) <= PARTY_MAX: return data
    return data

DISPUTE = os.path.join(HERE, 'character_dispute.json')
SET_ASIDE_DAYS = 7      # .rejected / .bad copies (the server turned them down, or unreadable), kept a few days to look
                        # at why. Records still to send wait as long as it takes: signed out for weeks, they still go up.

def game_server():
    """The game server (region) you play on, as the game names it - e.g. 'Aniimo-Americas' - from the game's own
    settings in the registry (HKCU/Software/Aniimo/Aniimo): the account's 'oversea_serverName' whose server id is the
    current one, else the plain 'serverName'. None when the game hasn't stored one."""
    try:
        import winreg
        values = {}
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r'Software\Aniimo\Aniimo') as key:
            for i in range(10000):
                try: name, value, _ = winreg.EnumValue(key, i)
                except OSError: break
                values[name.rsplit('_h', 1)[0]] = value.rstrip(b'\0').decode('utf-8', 'replace') if isinstance(value, bytes) else value
    except OSError: return None
    current = values.get('serverId')
    for name, value in values.items():                   # oversea_serverName_<account> + oversea_serverId_<account>
        if name.startswith('oversea_serverName_') and value:
            if current is None or values.get('oversea_serverId_' + name[len('oversea_serverName_'):]) == current:
                return str(value)[:40]
    return str(values['serverName'])[:40] if values.get('serverName') else None

def _read_json(path):
    try:
        with open(path, encoding='utf-8') as f: return json.load(f)
    except (OSError, ValueError): return None

def _unchanged(path, raw):
    """The queued file still holds what was read from it (not rewritten by a newer record while it was sending)."""
    try:
        with open(path, encoding='utf-8') as f: return f.read() == raw
    except OSError: return False

def _form(fields, files):
    """multipart/form-data from text fields and (name, filename, type, bytes) files."""
    boundary = uuid.uuid4().hex
    parts = [f'--{boundary}\r\nContent-Disposition: form-data; name="{k}"\r\n\r\n'.encode() + str(v).encode() + b'\r\n'
             for k, v in fields.items() if v is not None]
    for name, filename, ctype, data in files:
        parts.append(f'--{boundary}\r\nContent-Disposition: form-data; name="{name}"; filename="{filename}"\r\n'
                     f'Content-Type: {ctype}\r\n\r\n'.encode() + data + b'\r\n')
    parts.append(f'--{boundary}--\r\n'.encode())
    return b''.join(parts), f'multipart/form-data; boundary={boundary}'

def _multipart(payload, files):
    """The record and its proof pictures: {'evidence': bytes, 'evidence_full': bytes, 'battle': ..., 'battle_full': ...}."""
    boundary = uuid.uuid4().hex
    parts = [f'--{boundary}\r\nContent-Disposition: form-data; name="submission"\r\n'
             f'Content-Type: application/json\r\n\r\n'.encode() + json.dumps(payload).encode() + b'\r\n']
    for field, data in files.items():
        if data:
            parts.append(f'--{boundary}\r\nContent-Disposition: form-data; name="{field}"; filename="{field}.jpg"\r\n'
                         f'Content-Type: image/jpeg\r\n\r\n'.encode() + data + b'\r\n')
    parts.append(f'--{boundary}--\r\n'.encode())
    return b''.join(parts), f'multipart/form-data; boundary={boundary}'

class Submitter(threading.Thread):
    def __init__(self):
        super().__init__(daemon=True); self.stop = False; self.wake = threading.Event()
        os.makedirs(QUEUE, exist_ok=True)
        self.config = load_config(); self.config_stamp = self._stamp()
        self.linked = None                   # (uid, name, proven) last linked, so a character is sent once per change
        self.dispute = _read_json(DISPUTE)   # another account proved our UID first: what a dispute would send
        self.last_result = None              # shown in the app: (time, text)
        log.info('leaderboard submissions: ' + (f"on ({self.config['api_url']})" if self.config else 'off (opt in from the app)'))
        self.retry_big_proofs()

    @staticmethod
    def retry_big_proofs():
        """Character links the site turned down for their proof's size (over its old 512 KB, the Profile screen at 4K;
        it takes 1.9 MB now) go again, once: without that link the character stayed unproven, and its online status was
        refused too (409)."""
        try: names = os.listdir(QUEUE)
        except OSError: return
        for name in names:
            if not (name.startswith('character__') and name.endswith('.json.rejected')): continue
            path = os.path.join(QUEUE, name)
            item = _read_json(path)
            proof = os.path.join(HERE, item['proof']) if isinstance(item, dict) and item.get('proof') else None
            try: size = os.path.getsize(proof) if proof else 0
            except OSError: size = 0
            if not (512 * 1024 < size <= 1900 * 1000) or os.path.exists(path[:-len('.rejected')]): continue
            try: os.replace(path, path[:-len('.rejected')]); log.info(f'leaderboard: {name[:-14]} sent again (its proof is {size // 1024} KB, now allowed)')
            except OSError: pass

    @staticmethod
    def _stamp():
        try: return os.stat(C.SETTINGS_FILE).st_mtime
        except OSError: return None

    def refresh(self):
        """The leaderboard settings again when settings.json changed: turning sending or linking off, or signing out
        (or in with another account), applies at once (they were read once and kept until the meter restarted)."""
        stamp = self._stamp()
        if stamp != self.config_stamp:
            self.config_stamp = stamp
            try: self.config = load_config()
            except Exception: pass                                           # a save in progress: next time

    # -- queueing (called from the meter thread)
    def enqueue(self, payload, evidence_path=None, battle=None, party=None):
        """Queue one record; the same kind + run id replaces an older queued copy. Nothing is queued while off."""
        self.refresh()
        if not self.config: return
        payload = {**payload, 'clientVersion': CLIENT_VERSION}
        if 'server' not in payload: payload['server'] = game_server()      # read each time: you can switch servers
        name = _safe(f"{payload['kind']}__{payload['clientRunId']}") + '.json'
        write_json(os.path.join(QUEUE, name), {'endpoint': 'submissions', 'payload': payload,
                    'evidence': evidence_path if self.config['screenshots'] else None,
                    'battle': battle if self.config['screenshots'] else None,
                    'party': party if self.config['screenshots'] else None, 'queued_at': time.time()}, indent=1)
        log.info(f"leaderboard: queued {payload['kind']} {payload['clientRunId']}")
        self.wake.set()

    def link_character(self, uid, name, proof=None):
        """Link your character: the UID from the game log and your name. With proof (the game menu's name and UID, see
        live.game_menu) the site trusts it; without, it only fills in until a proof arrives."""
        self.refresh()
        if not self.config or not self.config['link_character'] or not uid or (uid, name, bool(proof)) == self.linked: return
        if self.linked and self.linked[:2] == (uid, name) and self.linked[2] and not proof: return
        self.linked = (uid, name, bool(proof))
        write_json(os.path.join(QUEUE, _safe(f'character__{uid}') + '.json'), {'endpoint': 'characters', 'payload': {'gameUid': uid, 'name': name},
                    'proof': os.path.relpath(proof, HERE) if proof else None, 'queued_at': time.time()})
        log.info(f'leaderboard: queued character link {name} (UID {uid}){" with the game menu as proof" if proof else ""}')
        self.wake.set()


    def character_stats(self, uid, stats):
        """Your character's Collection Progress from the game's Profile screen (live.profile_stats), or its level (from
        the EXP card), for your profile. Sent after the character's link (queue files go in name order), and only for a
        character you proved. The two queue separately, so a level doesn't replace numbers still waiting to go."""
        self.refresh()
        if not self.config or not self.config['link_character'] or not uid: return
        name = f'level__{uid}' if 'level' in stats else f'stats__{uid}'
        write_json(os.path.join(QUEUE, _safe(name) + '.json'), {'endpoint': 'characters/stats', 'payload': {'gameUid': uid, 'stats': stats}, 'queued_at': time.time()})
        log.info(f'leaderboard: queued profile numbers for UID {uid}')
        self.wake.set()

    def presence(self, uid, state, wait=False):
        """Online / AFK / offline for your profile (presence.py), sent straight away, not queued: an old one is no use.
        Only for a linked character. The site counts it for 5 minutes, and keeps none if the player turned it off on
        their profile (Edit profile > Show my online status)."""
        self.refresh()
        if not self.config or not self.config['link_character'] or not uid: return
        def go():
            body = json.dumps({'gameUid': uid, 'state': state}).encode()
            req = urllib.request.Request(f"{self.config['api_url']}/leaderboard/characters/presence", data=body, method='POST',
                                         headers={'authorization': f"Bearer {self.config['token']}", 'content-type': 'application/json',
                                                  'user-agent': CLIENT_VERSION})
            try:
                with urllib.request.urlopen(req, timeout=10): pass
            except (OSError, ValueError) as e:
                log.debug(f'presence: {state} not sent: {e!r}')
        if wait: go()
        else: threading.Thread(target=go, daemon=True, name='presence').start()

    def drop_proof(self, item):
        """A proof picture is only kept until it's sent (Captures & logs > Keep screenshots: until their data is saved)."""
        if int(C.load()['capture'].get('keep_hours', 0)) != 0: return
        for key in ('evidence', 'battle', 'party'):
            if item.get(key):
                try: os.remove(os.path.join(HERE, item[key]))
                except OSError: pass

    def pending(self):
        os.makedirs(QUEUE, exist_ok=True)
        return sum(1 for n in os.listdir(QUEUE) if n.endswith('.json'))

    # -- sending (background)
    def send(self, item):
        body, ctype = None, 'application/json'
        evidence = None
        if item.get('endpoint') in ('characters', 'disputes') and item.get('proof'):
            # a character link or dispute with the game menu as proof
            try:
                with open(os.path.join(HERE, item['proof']), 'rb') as f: proof = f.read()
            except OSError: proof = None
            if proof:
                jpeg = item['proof'].lower().endswith('.jpg')                 # large proofs are JPEGs (live.save_proof)
                body, ctype = _form(item['payload'], [('proof', 'menu.jpg' if jpeg else 'menu.png', 'image/jpeg' if jpeg else 'image/png', proof)])
                path = 'characters/disputes' if item['endpoint'] == 'disputes' else 'characters'
                req = urllib.request.Request(f"{self.config['api_url']}/leaderboard/{path}", data=body, method='POST',
                                             headers={'authorization': f"Bearer {self.config['token']}", 'content-type': ctype,
                                                      'user-agent': CLIENT_VERSION})
                with urllib.request.urlopen(req, timeout=30) as r: return r.status, json.loads(r.read() or b'{}')
        files = {}; payload = item['payload']
        boxes = PROOF_BOXES.get(payload.get('kind'), {})
        for key in ('evidence', 'battle'):
            if not item.get(key) or (key == 'battle' and 'evidence' not in files): continue   # Battle Data goes with a result
            path = os.path.join(HERE, item[key])
            try:
                files[key] = _crop_bytes(path, boxes[key]) if key in boxes else _evidence_bytes(path)
                files[key + '_full'] = _full_bytes(path)
            except (OSError, ValueError): files.pop(key, None)
        if item.get('party') and 'evidence' in files:            # the Party screen, whole: the run's lineup and buffs
            try: files['party'] = _party_bytes(os.path.join(HERE, item['party']))
            except (OSError, ValueError): pass
        if files:
            if boxes: payload = {**payload, 'extra': {**(payload.get('extra') or {}), 'proofBoxes': {k: list(v) for k, v in boxes.items() if k in files}}}
            body, ctype = _multipart(payload, files)
        else: body = json.dumps(payload).encode()
        endpoint = item['endpoint'] if item.get('endpoint') in ('characters', 'characters/stats', 'disputes') else 'submissions'
        req = urllib.request.Request(f"{self.config['api_url']}/leaderboard/{endpoint}", data=body, method='POST',
                                     headers={'authorization': f"Bearer {self.config['token']}", 'content-type': ctype,
                                              'user-agent': CLIENT_VERSION})
        with urllib.request.urlopen(req, timeout=30) as r: return r.status, json.loads(r.read() or b'{}')

    @staticmethod
    def expire():
        """Set-aside copies after SET_ASIDE_DAYS (records still to send are never dropped)."""
        try: names = os.listdir(QUEUE)
        except OSError: return
        now = time.time()
        for name in names:
            path = os.path.join(QUEUE, name)
            if not name.endswith(('.rejected', '.bad')): continue
            try:
                if now - os.path.getmtime(path) > SET_ASIDE_DAYS * 86400:
                    os.remove(path); log.info(f'leaderboard: removed {name} (set aside over {SET_ASIDE_DAYS} days ago)')
            except OSError: pass

    def run(self):
        delay = 5
        while not self.stop:
            self.wake.wait(delay); self.wake.clear()
            self.refresh()                                           # sending turned on or off, signed in or out
            self.expire()
            if not self.config: delay = 60; continue
            os.makedirs(QUEUE, exist_ok=True)
            for name in sorted(os.listdir(QUEUE)):
                if not name.endswith('.json'): continue
                path = os.path.join(QUEUE, name)
                try:
                    with open(path, encoding='utf-8') as f: raw = f.read()
                    try: item = json.loads(raw)
                    except ValueError:                                       # unreadable: set aside, not a jam
                        log.warning(f'leaderboard: {name} is unreadable, set aside'); os.replace(path, path + '.bad'); continue
                    if not isinstance(item, dict): os.replace(path, path + '.bad'); continue
                    status, reply = self.send(item)
                    log.info(f"leaderboard: sent {name} -> {status} {reply.get('submissionId') or reply.get('gameUid') or reply.get('disputeId')}"
                             + (' (updated)' if reply.get('updated') else ''))
                    if item.get('endpoint') == 'disputes' or (item.get('endpoint') == 'characters' and item.get('proof')):
                        self.dispute = None                                  # sent, or the link went through after all
                        try: os.remove(DISPUTE)
                        except OSError: pass
                    self.last_result = (time.time(), f'sent {name[:-5]}')
                    if _unchanged(path, raw): os.remove(path); self.drop_proof(item)
                    else: self.wake.set()           # replaced while it was sending (a newer read): send that one too
                except urllib.error.HTTPError as e:
                    msg = e.read()[:300].decode('utf-8', 'replace')
                    if e.code == 409 and item.get('endpoint') == 'characters' and item.get('proof') and '"dispute":true' in msg.replace(' ', ''):
                        # another account proved this UID first: the app offers to send a dispute with our proof
                        self.dispute = {'gameUid': item['payload']['gameUid'], 'name': item['payload']['name'], 'proof': item['proof']}
                        with open(DISPUTE, 'w', encoding='utf-8') as f: json.dump(self.dispute, f)
                    if e.code in (400, 409, 413):                             # the server will never accept it
                        log.warning(f'leaderboard: rejected {name}: {e.code} {msg}')
                        self.last_result = (time.time(), f'rejected {name[:-5]}: {msg[:120]}')
                        os.replace(path, path + '.rejected'); self.drop_proof(item)
                    else:                                                     # auth/gate/server trouble: keep it
                        log.warning(f'leaderboard: {name} not sent yet: {e.code} {msg}')
                        self.last_result = (time.time(), f'waiting: server said {e.code}'); break
                except (OSError, ValueError) as e:
                    log.warning(f'leaderboard: {name} not sent yet: {e!r}'); break
                except Exception as e:                                       # an odd reply: never stop the sender
                    log.warning(f'leaderboard: {name} not sent ({e!r})'); break
            delay = 60 if any(n.endswith('.json') for n in os.listdir(QUEUE)) else 300

# ---------------------------------------------------------------- payloads from the meter's records
def _slot(cycle, boss_id):
    """{'slot': 'left' | 'middle' | 'right', 'split': the week's ranking period} for a run's extra, from the week list."""
    import reader
    slot, split = reader.cycle_slot(cycle, boss_id)
    return {k: v for k, v in (('slot', slot), ('split', split)) if v is not None}

def _lineup(summary):
    """{'lineup': each slot's player and lead Aniimo from the Party screen, 'title': your title} for a run's extra."""
    # every player's card: lead Aniimo, level and CP (shown on the site, the user's call 2026-10-31), and the rest of
    # the team from the pictures in the card's team row (icons.py; unread ones left out)
    teams = summary.get('teams') or []
    def entry(m):
        e = {'slot': m.get('slot'), 'name': (m.get('name') or '')[:40] or None, 'ai': m.get('ai'), 'self': True if m.get('self') else None}
        lead = {'aniimo': (m.get('aniimo') or '')[:40] or None, 'aniimoLevel': m.get('aniimo_level'), 'cp': m.get('cp')}
        lead = {k: v for k, v in lead.items() if v is not None}
        slot = m.get('slot')
        team = teams[slot] if isinstance(slot, int) and slot < len(teams) and teams[slot] else             summary.get('team') if m.get('self') else None          # a run saved before every card was read
        rest = [{'aniimo': n[:40]} for n in team or [] if n]
        e.update(**lead, **({'team': [lead] + rest} if lead.get('aniimo') or rest else {}))
        return {k: v for k, v in e.items() if v is not None}
    lineup = [entry(m) for m in summary.get('party') or []]
    return {'lineup': lineup} if lineup else {}

def run_payload(summary, uid, duration=180):
    """interlink_run from a logs/fights/<fight>.json summary (result screen + Battle Data + meter)."""
    res = summary.get('result') or {}
    bd = summary.get('battle_data') or {}
    meter = summary.get('meter') or {}
    players = []
    cards = bd.get('players') or []
    for i, p in enumerate(res.get('players') or [{} for _ in cards]):
        c = cards[i] if i < len(cards) else {}
        players.append({k: v for k, v in {
            'name': (p.get('name') or '')[:40] or None, 'stat': (p.get('stat') or '')[:60] or None, 'value': p.get('value'),
            'ai': p.get('ai') if isinstance(p.get('ai'), bool) else None,
            'self': True if p.get('self') else None,
            'avgDps': c.get('avg_dps'), 'breakValue': c.get('break'), 'damageTaken': c.get('taken'), 'heal': c.get('heal'),
        }.items() if v is not None})
    secs = res.get('time_s')
    started = summary.get('started')
    try: played = int(time.mktime(time.strptime(started, '%Y-%m-%d %H:%M:%S'))) if started else None
    except ValueError: played = None
    return {
        'kind': 'interlink_run', 'clientRunId': os.path.splitext(meter.get('csv') or 'run')[0],
        **({'server': summary['server']} if summary.get('server') else {}),
        'gameUid': summary.get('uid') or uid, 'cycle': summary.get('cycle'),
        'boss': {'id': summary.get('puppetId'), 'name': summary.get('boss'), 'level': summary.get('level')},
        'score': res.get('score'), 'stars': res.get('rating_stars'), 'clearTimeS': secs,
        'cleared': None if secs is None else res['cleared'] if 'cleared' in res else secs < duration,
        'teamDps': bd.get('team_avg_dps'), 'meterDps': meter.get('dps'), 'meterDamage': meter.get('damage'),
        'partySize': len(players) or None, 'players': players, 'playedAt': played,
        # the leader's Season Buffs and Special Offering from the Party screen's icons: [{id, name, level}]
        **({'buffs': [{k: v for k, v in b.items() if v is not None} for b in summary['buffs']]} if summary.get('buffs') else {}),
        'extra': {'newBest': res.get('new_best'), 'newBests': summary.get('new_bests') or [], 'trackedFromStart': meter.get('tracked_from_start'),
                  'instanceId': summary.get('instance'), **_slot(summary.get('cycle'), summary.get('puppetId')),
                  **_lineup(summary), **({'starCap': res['star_cap']} if res.get('star_cap') else {}),
                  **({'endedByLeader': True} if res.get('ended_by_leader') else {})},
    }

def score_payload(screen, boss, score, stars, uid, cycle, boss_id=None):
    """interlink_week (Challenge Stats) or interlink_best (battle prep) - one entry per week (per boss)."""
    week = cycle or time.strftime('%Y-W%W')
    if screen == 'challenge_stats':
        return {'kind': 'interlink_week', 'clientRunId': _safe(f'week-{week}'), 'gameUid': uid, 'cycle': cycle,
                'score': score, 'stars': stars, 'playedAt': int(time.time())}
    return {'kind': 'interlink_best', 'clientRunId': _safe(f'best-{week}-{boss_id or boss}'), 'gameUid': uid,
            'cycle': cycle, 'boss': {'id': boss_id, 'name': boss[:80]}, 'score': score, 'stars': stars,
            'playedAt': int(time.time()), 'extra': _slot(cycle, boss_id)}

def sim_payload(sim, uid, played_at=None):
    """sim_run: one Holo-Battle Sim win from its Victory screen - the battle time, the stage as shown ("Training of
    Water", "Trial of Fire") and its difficulty ("Hard", "Challenge"). One entry per win."""
    at = int(played_at or time.time())
    return {'kind': 'sim_run', 'clientRunId': _safe(f'sim-{uid or "x"}-{at}'), 'gameUid': uid,
            'clearTimeS': sim['time_s'], 'trial': sim['trial'], 'difficulty': sim['difficulty'], 'cleared': True,
            'playedAt': at, 'extra': {'trial': sim['trial'], 'element': sim['element']}}

def egg_match_payload(match, season, uid, played_at=None):
    """eggheist_match: one Egg Heist match from the end screen - Loot This Match (score), time, escaped or not,
    difficulty and Total Defeats. One entry per match."""
    at = int(played_at or time.time())
    return {'kind': 'eggheist_match', 'clientRunId': _safe(f'egg-{uid or "x"}-{at}'), 'gameUid': uid, 'cycle': season,
            'score': match.get('loot'), 'clearTimeS': match.get('time_s'),
            'cleared': None if match.get('outcome') not in ('success', 'failed') else match['outcome'] == 'success',
            'difficulty': (match.get('difficulty') or None) if re.fullmatch(r'[A-Za-z ]{1,20}', match.get('difficulty') or '') else None,
            'defeats': match.get('defeats'), 'playedAt': at,
            'extra': {k: match.get(k) for k in ('mode', 'objective', 'chests_today') if match.get(k)}}

def eggheist_payload(standing, season, uid, profile=None):
    """eggheist_season: the season standing as seen (Rank Info, or the rank screen after a match) - one entry per
    season, updated as it changes. The rank goes on the season's ladder; `profile` is for the player's profile:
    nextRankAt (Rank Info's "1319/2370"), and the last match's gain and bar ("1337/1350 +18")."""
    s = season or time.strftime('%Y')
    return {'kind': 'eggheist_season', 'clientRunId': _safe(f'eggheist-{s}'), 'gameUid': uid, 'cycle': season,
            'rank': {'tier': standing['tier'], 'subtier': standing['subtier'], 'name': standing['name'][:40]},
            'score': standing['points'], 'stars': standing['stars'], 'playedAt': int(time.time()),
            'extra': {k: v for k, v in (profile or {}).items() if v is not None}}
