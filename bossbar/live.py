"""Aniimo live boss DPS meter (team damage from the boss HP bar).

Watches the game window through the aLogs app's shared capture (on the graphics card, wincapture.py), reads the
boss bar, and shows team damage and DPS in a small always-on-top overlay. It also follows the
game's own Player.log for scene changes, which mark exactly when a boss attempt starts and ends.
It only looks at screen pixels and the game's text log; it never touches the game's memory or files.

Usage:
    python live.py [--interval 0.05] [--no-overlay] [--hide-from-capture] [--no-debug-frames] [--replay shots...]
"""
from core import *
# the core's parts and the leaderboards plug-in; their names are re-exported here too (the tests use live.<name>)
from fights import *
from results import *
from leaderboards import *
import devflags as D
# the DPS meter plug-in: its overlay
from dps_overlay import run_overlay


class Meter(FightsMixin, ResultsMixin, LeaderboardsMixin):
    # the reading loop and what every plug-in shares; following fights is in fights.py, reading end screens in
    # results.py, the leaderboards plug-in in leaderboards.py
    def __init__(self, args, out):
        self.args, self.out = args, out
        self.switches = SwitchWatch()                    # each plug-in's switch, to give a stopped one another go
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
        self.hold = InputBlock(log.info); self.focus_guard = FocusGuard(log.info); self.hold_started = 0.0; self.clicks = 0; self.unread = 0; self.last_click = 0.0
        self.numbers = None             # the damage-number recorder (experimental), made when the option is on
        try: self.cfg_stamp = os.stat(C.SETTINGS_FILE).st_mtime   # the settings as loaded at start (a save during start-up was missed)
        except OSError: pass
        self.auto_until = 0.0        # automatic "View Battle Data" session running until this time
        self.last_status = 0.0; self.status_text = 'starting'
        self.submitter = None
        self.stop_handles = [] if args.replay else stop_handles()
        if not args.replay and not args.no_submit:
            self.submitter = S.Submitter(); self.submitter.on_sent = lambda payload, reply: (self.sent_notice(payload, reply), self.result_sent(payload)); self.submitter.start()
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
        self.picture_gap(im is None, time.time())
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
            self.dev_line(now)
            self.overlay_check()

    def picture_gap(self, missing, now):
        """Pictures stopping for 2 s or more is logged with the reader's reason (wincapture SharedCapture.why), again
        every 5 s while it lasts, and when they come back: on 6 Nov the meter had no picture for 18 s on a result
        screen, so the automatic Battle Data click came 22 s late, and the log only said "can't see the game window"."""
        since = getattr(self, 'no_picture_since', 0.0)
        if not missing:
            if since and now - since >= 2: log.info(f'game picture back after {now - since:.1f} s')
            self.no_picture_since = 0.0; return
        if not since: self.no_picture_since = since = now
        if now - since >= 2 and now - getattr(self, 'no_picture_said', 0.0) >= 5:
            self.no_picture_said = now
            log.info(f'no game picture for {now - since:.1f} s: {wc.shared.why or "no reason given"} (source {wc.shared.source or "none"})')

    def overlay_check(self):
        """Discord's in-game overlay draws inside the game's own picture (DiscordHook64.dll), so no capture can leave
        it out: its voice widget (names, avatars) went into proof pictures sent to the site (6 Nov). Once a minute
        while capturing; said once per start (log and a notice), so the player can turn it off for Aniimo."""
        if getattr(self, 'discord_said', False) or not self.hwnd: return
        try: mods = wc.window_modules(self.hwnd)
        except Exception: return
        if 'discordhook64.dll' not in mods and 'discordhook.dll' not in mods: return
        self.discord_said = True
        log.info("Discord's in-game overlay is loaded in the game: it shows in every capture, proof pictures included")
        self.out('toast', {'head': "Discord's overlay shows in proof pictures", 'detail':
                           'Discord › Settings › Game Overlay: turn it off for Aniimo', 'accent': '#ffc861',
                           'screen': None, 'kind': 'notice'})        # (a dict: never folded into "N records sent")

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

    # The screens' text boxes: (name, preparation, box as fractions of the 16:9 UI area: x0, y0, x1, y1)
    SCREEN_BOXES = (('h', 'text', (0.065, 0.035, 0.17, 0.08)), ('cs', 'text', (0.35, 0.075, 0.65, 0.155)),
                    # the Party screen's top line: "Operation: Egg Heist Team Mode ...", which has its own layout (EGG_PARTY_SLOTS)
                    ('ph', 'text', (0.39, 0.005, 0.60, 0.045)),
                    ('bs', 'text', (0.785, 0.03, 0.97, 0.09)), ('lbl', 'text', (0.2, 0.425, 0.83, 0.47)),
                    ('cc', 'text', (0.38, 0.07, 0.62, 0.125)), ('me', 'text', (0.657, 0.068, 0.86, 0.104)),
                    ('uid', 'text', (0.657, 0.104, 0.745, 0.134)), ('eh', 'bright', (0.03, 0.08, 0.52, 0.17)),
                    # profile notes (overworld): the EXP card at the top and the catch card on the left
                    ('pl', 'bright', (0.455, 0.118, 0.545, 0.195)), ('pe', 'bright', (0.44, 0.222, 0.56, 0.278)),
                    ('pc', 'bright', (0.055, 0.485, 0.16, 0.555)), ('pb', 'bright', (0.05, 0.555, 0.17, 0.595)),
                    # Egg Heist's rank screen after a match: "1337/1350" right of the badge (then "+18" next to it)
                    ('ep', 'text', (0.72, 0.555, 0.83, 0.605)),
                    # ...and its line at the bottom, "Tap anywhere to continue", the sign of that screen. With a controller the
                    # prompt has a button in it and it all sits higher, so this box takes in both (Meter.egg_rank_gain)
                    ('et', 'text', (0.36, 0.885, 0.64, 0.955)),
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
    PLACE_SKIP = {'interlink': {'ph', 'cc', 'pl', 'pe', 'pc', 'pb', 'eh', 'ep', 'es', 'et', 'sv'},
                  'holo-sim': {'ph', 'cs', 'lbl', 'bs', 'cc', 'pc', 'pb', 'eh', 'ep', 'es', 'et'},
                  'egg heist': {'ph', 'cs', 'lbl', 'bs', 'cc', 'sv', 'pc', 'pb'},
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
        # the automatic Battle Data session waits for the result screen to be wholly read before its first click: read it
        # 4 times a second until then, not every 1.5 s (6 Nov: the click came ~4 s after the screen, 1.5 s of it this wait)
        if self.auto_active() and getattr(self, 'clicks', 1) == 0: gap = min(gap, 0.25)
        # Egg Heist's end and rank screens, read with the player's inputs held: as soon as the reads can agree (9 Nov: three
        # reads 1.6 s apart held them about 3.5 s, on a still screen)
        if getattr(self, 'egg_hold', None) or getattr(self, 'egg_read', None) or getattr(self, 'egg_points', None): gap = min(gap, 0.25)
        if not quick and self.idle(): gap = max(gap, 1.0)
        # Dev switch busy-half-pace: busy with no boss bar and nothing that can't wait (busy_calm), 2 reads a second
        elif not quick and gap < self.CALM_READ_S and D.on('busy-half-pace') and self.busy_calm(now): gap = self.CALM_READ_S
        return now - self.last_results >= gap

    CALM_LOOK_S, CALM_READ_S = 0.1, 0.5         # busy-half-pace: seconds between looks, and between screen reads
    CALM_JUMP_S, CALM_BLACK_S = 1.5, 10.0       # ...and today's pace for this long after the top part jumped / went black

    def busy_calm(self, now):
        """Dev switch busy-half-pace. Busy with no boss bar (an Egg Heist run, an Interlink room) used today's full
        pace throughout: 20 looks and up to 10 screen reads a second. Calm is when nothing there can't wait: no screen
        just spotted and no fade just seen (calm_watch), no screen up, no hold of the player's inputs, automatic
        click, tour, result being read, kept or sent, names being read, and not the time after a fight (45 s) or an Egg
        Heist match (90 s), when result and rank screens come."""
        if self.fight or self.bar is not None or self.args.replay is not None or self.idle(): return False
        if now < max(getattr(self, 'calm_until', 0), getattr(self, 'full_until', 0)): return False
        if (self.end_sig is not None or self.auto_until or self.pending_run or self.awaiting or self.send_waiting
                or self.hold.active or self.holosim_space or self.names_waiting()): return False
        if any(getattr(self, k, None) for k in ('egg_hold', 'egg_read', 'egg_points', 'egg_pending', 'egg_click', 'sim_tour', 'sim_ctl')):
            return False
        a = self.after_fight
        if a and now < a[1]: return False
        return now - max(getattr(self, 'egg_end_seen', 0), getattr(self, 'egg_sent_at', 0)) >= self.EGG_RANK_WAIT_S

    def calm_pace(self, interval):
        self.dev_count('calm'); return interval if self.bossrush_space else max(interval, self.CALM_LOOK_S)

    def calm_watch(self, sig, now):
        """busy-half-pace, every look with no boss bar: a screen opening shows as a jump of the frame's top part from
        one look to the next, and Egg Heist's end screen comes out of a fade to black (egg_fade's test of black, on the
        top part). Either one puts today's pace back, for CALM_JUMP_S and CALM_BLACK_S."""
        last, self.calm_sig = getattr(self, 'calm_sig', None), sig
        hold = self.CALM_JUMP_S if last is None or len(last) != len(sig) or screen_changed(sig, last) else 0.0
        lum = sorted(sig)
        if lum[len(lum) // 2] < 14 and lum[int(len(lum) * 0.95)] < 40: hold = self.CALM_BLACK_S
        if hold: self.calm_until = max(getattr(self, 'calm_until', 0), now + hold)

    def top_sig_fast(self, im, top_h, now):
        """The top part's signature picked straight off the frame (core.top_sig_direct), not
        from a copy of the top part. The same bytes by construction; checked against the old way on the first look of
        each frame size and once a minute after, and if the two ever differ the old way is used until the meter restarts."""
        st = self.__dict__.setdefault('sig_check', {'bad': False, 'seen': {}}); key = (im.size, top_h)
        if not st['bad'] and now - st['seen'].get(key, -1e9) < 60:
            self.dev_count('sig_direct'); return top_sig_direct(im, top_h)
        old = screen_sig(im.crop((0, 0, im.width, top_h)))
        if st['bad']: return old
        if len(st['seen']) > 64: st['seen'].clear()
        st['seen'][key] = now
        if top_sig_direct(im, top_h) != old:
            st['bad'] = True; log.info(f'dev top-sig-direct: the two ways differ at {im.width}x{im.height}, top {top_h}: the old way from here on')
        return old

    def search_rests(self, now):
        """Dev switch tabbed-out-rest: tabbed out with no fight, the whole-frame bar search runs twice a second, not on
        every look. With red or orange on screen where no bar is, it took 85-110 ms a look: a third of a core over a
        tabbed-out Interlink session (10 Nov). The spots a bar had before are still checked on every look."""
        if self.fight or self.bar is not None or self.args.replay is not None or not self.hwnd: return False
        if not D.on('tabbed-out-rest') or wc.is_foreground(self.hwnd): return False
        if now - getattr(self, 'searched_at', 0) >= 0.5: self.searched_at = now; return False
        self.dev_count('search_rest'); return True

    def dev_count(self, key, by=1):
        n = self.__dict__.setdefault('dev_n', {}); n[key] = n.get(key, 0) + by

    def dev_line(self, now):
        """Once a minute, with the load line: what the look loop's Dev switches did since the last line (counts and
        milliseconds only), so a tester's log shows each one working; nothing while none of them did anything. Also
        where the OCR helpers already running follow helpers-below-normal (new ones start that way: Ocr.launch)."""
        try:
            low = D.on('helpers-below-normal')
            if low != getattr(self, 'helpers_low', False):
                self.helpers_low = low
                hs = [o for o in (self.ocr, getattr(self.ocr_bg, 'ocr', None), getattr(self.screen_bg, 'ocr', None),
                                  getattr(self, 'hp_ocr', None)) if getattr(o, 'set_priority', None)]
                log.info(f"dev helpers-below-normal: {sum(bool(o.set_priority(low)) for o in hs)} of {len(hs)} OCR helpers now at "
                         f"{'Below Normal' if low else 'Normal'} priority")
            w = WRITER.n; n = getattr(self, 'dev_n', None) or {}
            cur = {'pictures queued': w['queued'], 'pictures saved in the look': w['inline'], 'pictures failed': w['failed'],
                   'calm looks': n.get('calm', 0), 'whole frames after a match': n.get('quiet_whole', 0),
                   'top parts after a match': n.get('quiet_top', 0), 'bar searches skipped': n.get('search_rest', 0),
                   'area requests': wc.shared.asked, 'for more rows': wc.shared.grown}
            last, self.dev_last = getattr(self, 'dev_last', None) or dict.fromkeys(cur, 0), cur
            d = {k: v - last.get(k, 0) for k, v in cur.items()}
            if not D.on('area-keep-short'): d['area requests'] = d['for more rows'] = 0     # (counted either way)
            ms, self.dev_ms = w['ms'] - getattr(self, 'dev_ms', 0.0), w['ms']
            if any(v > 0 for v in d.values()):
                log.info('dev: ' + ', '.join(f'{v} {k}' for k, v in d.items() if v > 0)
                         + (f", pictures took {ms:.0f} ms off the look (longest {w['max']:.0f})" if d['pictures queued'] > 0 else ''))
            w['max'] = 0.0
        except Exception as e:
            log.info(f'dev: line not written ({e!r})')

    def full_frame_due(self, now):
        """Between fights only the frame's top part is copied (where a boss bar appears; ~3x cheaper at 4K) unless
        the whole frame is needed: for a screen read, the end-screen note, Egg Heist's fade or diagnostic frames."""
        # In an Egg Heist run, whole frames were copied on every look (20 a second, ~33 MB each at 4K) for the fade hold,
        # which is gone; the end and rank screens bring whole frames themselves (screen reads, egg_sent_at). Dev switch
        # egg-top-only: only the top part and the screen-read boxes there (10 Nov: a player's game lost FPS in runs).
        if self.end_sig is not None or getattr(self, 'egg_hold', None) or getattr(self, 'sim_tour', None): return True
        if self.sanctum_in_run(now) and not D.on('egg-top-only'): return True
        # the rank screen can come any moment after a match: whole frames on every look. Dev switch quiet-top-only: only
        # on the looks a screen read is made from (every read there still gets a whole frame: the rank screen is kept
        # from one), and for the leader's line after an Interlink fight; the looks in between copy the top part
        egg_after = now - getattr(self, 'egg_sent_at', 0) < self.EGG_AFTER_S
        if egg_after and not D.on('quiet-top-only'): return True
        if egg_after and getattr(self, 'last_fight', None) and now - getattr(self, 'fight_ended_at', 0) <= 20: return True
        if getattr(self, 'sim_ctl', None) or (self.holosim_space and (getattr(self, 'sim_gate_state', None) or {}).get('last')): return True
        cap = self.args.cfg['capture']; a = self.after_fight
        if a and cap['end_screens'] and now - a[2] >= 3: return True
        if cap['nobar_frames'] and self.bossrush_space and now - self.last_nobar >= 5 and self.nobar_count < 30: return True
        bg = getattr(self, 'screen_bg', None)
        if bg is None: return True
        # (after a match the read asks as check_battle_data will: at once while an automatic click waits for its screen)
        if not bg.busy and not self.names_waiting() and (self.screen_read_due(now, True) if egg_after and self.auto_active()
                                                         else self.screen_read_due(now)):
            # only for a screen read: just its boxes (screen_parts), unless a screen is up (then whole frames until it's gone)
            if egg_after: self.dev_count('quiet_whole')
            return True if egg_after or now < getattr(self, 'full_until', 0) else 'screen'
        if egg_after: self.dev_count('quiet_top')
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
    def title_is(head, word):
        """Whether a screen's title starts with this word, by its letters: OCR puts stray spaces in a short title (8 Nov:
        a Party screen's read 'p arty' for 35 s, so it was spotted once and never read: no teams or buffs that run)."""
        return re.sub(r'[^a-z]', '', (head or '').lower()).startswith(word)

    @staticmethod
    def screen_spotted(heads):
        """Which screen read_screen would act on (its own tests, roughly), or None: the cue for whole frames (B8)."""
        g = lambda k: (heads.get(k) or '').lower()
        head = g('h'); hl = re.sub(r'[^a-z]', '', head)
        tests = (('result', 'score' in g('lbl') and 'time' in g('lbl')), ('Holo-Battle Sim result', 'battle time' in g('sv')),
                 ('challenge', 'challenge' in g('cs')), ('battle prep', 'score' in g('bs')), ('Catch Countdown', 'catch' in g('cc')),
                 ('Egg Heist rank', 'rank info' in head), ('profile', Meter.title_is(head, 'profile')),
                 ('Egg Heist', 'adventure' in g('eh') or Meter.rank_screen(heads)),   # (before Party: the rank screen is over it)
                 ('party', Meter.title_is(head, 'party')), ('Holo-Battle', hl.startswith('holobattlei')),
                 ('menu', bool(g('me') and g('uid'))), ('Battle Data', 'battle' in hl and 'dat' in hl),
                 ('EXP card', 'level' in g('pl')), ('catch card', bool(re.search(r'view|close', g('pb'))) and '/' in g('pc')))
        return next((name for name, hit in tests if hit), None)

    PARTY_KEEP_S = 30      # a Party screen stays 'up' this long after its last sign, title or Environment info

    @staticmethod
    def rank_line(heads):
        """The line under Egg Heist's rank screen: "Tap anywhere to continue", or a controller's wording of it."""
        line = (heads.get('et') or '').lower()
        return 'continue' in line and ('tap' in line or 'press' in line)

    @staticmethod
    def rank_screen(heads):
        return (Meter.rank_line(heads) or re.fullmatch(r'\s*\d{1,6}\s*/\s*\d{1,6}\s*', heads.get('ep') or '') is not None
                or re.fullmatch(r'\s*[xX×]\s*\d{1,6}\s*', heads.get('es') or '') is not None)

    def party_up(self, heads, now):
        """Whether this read is of the Party screen. Its title decides, but on some backgrounds OCR reads it only now and
        then (8 Nov: once in 26 s, 20 s before the fight, so the offering chosen after that and any late team change were
        missed). So once the title has been read, 'Environment info' at the top right keeps the screen up until the fight
        starts, while the title reads as nothing or a garbled Party. A Squad screen shows that text too: it needs its own
        title gone, and a Party title just before."""
        head = heads.get('h') or ''
        if getattr(self, 'fight', None) is not None:
            self.party_title_at = 0; return False
        # Egg Heist's rank screen comes up over the Party screen, whose title and top line still read: it's the rank screen (8 Nov)
        if self.rank_screen(heads): return False
        if self.title_is(head, 'party'):
            self.party_title_at = now; return True
        # Egg Heist's Party screen says so across the top, however its title reads (8 Nov: seen for one read, and no party sent)
        if re.search(r'operation|heist', (heads.get('ph') or '').lower()):
            self.party_title_at = now; return True
        if now - getattr(self, 'party_title_at', 0) > self.PARTY_KEEP_S: return False
        letters = re.sub(r'[^a-z]', '', head.lower())
        if letters and difflib.SequenceMatcher(None, letters, 'party').ratio() < 0.6: return False     # another screen's title
        if not re.search(r'en[a-z]{0,3}ron', (heads.get('bs') or '').lower()): return False
        self.party_title_at = now
        if now - getattr(self, 'party_kept_said', 0) > 15:
            self.party_kept_said = now; log.info(f'Party screen: still up by Environment info (title read {head!r})')
        return True

    # A copy of the screen (Desktop Duplication, Windows 10) shows anything over the game, even a picture of a result screen. So
    # these screens are read from window capture, in bursts: the app opens it (Windows 10's yellow border with it) while one is
    # up, and closes it GAME_ONLY_HOLD_S after. It takes a moment to open: nothing is read from the screen meanwhile,
    # for up to GAME_ONLY_WAIT_S, and then it is, on a PC where it doesn't open.
    GAME_ONLY_SCREENS = {'result', 'Holo-Battle Sim result', 'challenge', 'battle prep', 'Egg Heist rank', 'profile', 'party', 'Egg Heist', 'menu', 'Battle Data'}
    GAME_ONLY_HOLD_S = 3.0
    GAME_ONLY_WAIT_S = 1.5
    game_only_since = 0.0; game_only_asked = 0.0; game_only_spot = None

    def game_only(self, spotted, now):
        """This frame of a spotted screen should wait for the game window alone. Asks for it, and says when the screen shows one of
        these and the game window, once it's open, doesn't (Meter.game_only_gone)."""
        if spotted not in self.GAME_ONLY_SCREENS: return False
        if now - self.game_only_asked > 0.5:
            self.game_only_asked = now
            try: wc.shared.set_game_only_until((time.time() + self.GAME_ONLY_HOLD_S) * 1000)
            except Exception: pass
        if not wc.shared.screen_copy:
            self.game_only_since = 0.0; self.game_only_spot = None; return False
        last = self.game_only_spot[1] if self.game_only_spot else 0.0
        if not self.game_only_since or now - last > self.GAME_ONLY_HOLD_S: self.game_only_since = now
        self.game_only_spot = (spotted, now)
        if now - self.game_only_since < self.GAME_ONLY_WAIT_S: return True
        if not getattr(self, 'game_only_said', False):
            self.game_only_said = True; log.info(f'window capture did not open for the {spotted} screen: reading the screen')
        return False

    def game_only_gone(self, spotted, now):
        """The screen showed one of these and, with the game window open, it's nothing like it: something on top, or a misread."""
        spot = self.game_only_spot
        if spot and not spotted and not wc.shared.screen_copy and now - spot[1] < self.GAME_ONLY_HOLD_S:
            self.game_only_spot = None
            log.info(f"the screen showed a {spot[0]} screen and the game window doesn't: nothing read from it")

    EGG_AFTER_S = 45.0     # after a match's end screen: whole frames, and pictures of what comes next, to see the rank screen

    def read_screen(self, im, now, heads):
        heads = self.rank_only_after_match(heads, now)
        self.egg_rank_watch(im, heads, now)
        self.screens_gone(now)
        self.egg_gone(now)
        if now - getattr(self, 'egg_sent_at', 0) < self.EGG_AFTER_S: keep_sample(im, 'egg_after', gap=3, kept=15)
        # a screen the meter reads is up: its readers look at more of the frame than the boxes. A frame of only the
        # boxes (screen_parts) is read again whole at once; whole frames go on until FULL_HOLD_S after the last one
        party = self.party_up(heads, now)
        spotted = self.screen_spotted(heads) or ('party' if party else None)
        self.game_only_gone(spotted, now)
        if spotted and self.args.replay is None:
            self.full_until = now + self.FULL_HOLD_S
            self.egg_screen_hold(spotted, heads, now)
            if self.game_only(spotted, now): self.last_results = 0; return
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
            # the first click once the screen is wholly read (result_ready), else RESULT_PAUSE in
            ready = self.clicks == 0 and getattr(self, 'result_ready', 0) >= self.hold_started
            if self.auto_active() and (now - self.last_click > 1.0 or ready):
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
        if self.title_is(head, 'profile'): self.test_frame(im, 'profile', now); self.profile_screen(im, now, u, ux, H); return
        if party:
            self.test_frame(im, 'party', now)
            self.party_layout(heads.get('ph', ''))
            self.party_circle(im, now, u, ux, H)
            # at party_screen's own pace (2 s): a pill's ready mark or voice icon changes its look, and each change
            # would send all four to the name models at every screen read, where the screen reads them every 2 s
            if now - getattr(self, 'party_seen', 0) >= 2:
                self.names_first(self.party_screen, (im, now, u, ux, H), self.party_names(im, u, ux, H), 20)
            return
        if re.sub(r'[^a-z]', '', head.lower()).startswith('holobattlei'): self.interlink_screen(im, now, u, ux, H); return
        if self.rank_line(heads) or re.fullmatch(r'\s*\d{1,6}\s*/\s*\d{1,6}\s*', heads.get('ep', '')): self.egg_rank_gain(im, now, u, ux, H)
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

    # Egg Heist's Party screen has up to three players, laid out its own way: you centre if you lead, 2P on the left, 3P on the
    # right (measured on 2-player and 3-player screens, 8 Nov), and no Special Offering or Season Buffs. The cards are the same
    # size as Interlink's, and their team rows the same.
    EGG_PARTY_SLOTS = ((0.44, 0.197, 0.545, 0.226), (0.143, 0.2515, 0.2525, 0.278), (0.73, 0.2267, 0.8275, 0.256))
    EGG_PARTY_CARDS = ((0.4505, 0.569, 0.5605, 0.626), (0.162, 0.6236, 0.272, 0.6806), (0.743, 0.5996, 0.853, 0.6566))
    egg_party = False

    def party_layout(self, top_line):
        """Which Party screen this is, by its top line. A line that didn't read leaves it as it was."""
        line = re.sub(r'[^a-z]', '', (top_line or '').lower())
        if not line: return
        self.egg_party = 'heist' in line or 'operation' in line
        self.__dict__.pop('PARTY_SLOTS', None); self.__dict__.pop('PARTY_CARDS', None)
        if self.egg_party: self.PARTY_SLOTS, self.PARTY_CARDS = self.EGG_PARTY_SLOTS, self.EGG_PARTY_CARDS

    CARD_LEAD = r"\s*([A-Za-z][A-Za-z' -]{1,24}?)\s*(?:/|$)"

    def own_slots(self, members, now):
        """Each player in a slot of their own. Badges fade as the fight nears (9 Nov: 4P read 'P', then 'IP', which is 1P, and took
        1P's place in the last read, so the run went with 3 of 4). So a CP seen moments ago keeps its slot, and of two pills
        after one slot, the one that sits in that place has it and the other takes a slot nobody holds."""
        recent = (self.party or {}).get('members', []) if self.party and now - self.party['at'] < 60 else []
        for m in members:
            was = [p for p in recent if m.get('cp') and p.get('cp') == m['cp']]
            if len(was) == 1: m['slot'] = was[0]['slot']
        kept = {}
        for m in sorted(members, key=lambda m: m.get('pill') != m['slot']):
            if m['slot'] not in kept: kept[m['slot']] = m
        for m in members:
            if any(m is v for v in kept.values()): continue
            free = [s for s in range(len(self.PARTY_SLOTS)) if s not in kept]
            if not free: continue
            m['slot'] = m['pill'] if m.get('pill') in free else free[0]
            kept[m['slot']] = m
        return list(kept.values())

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
        # (prep_bright pads 20 px. Each pill this screen has: Egg Heist's has three, and asking for a fourth stopped every read)
        t = self.ocr.read(crops, names={k: 20 for k in crops if re.fullmatch(r'n\d', k)})
        # no card reads at all: not a Party screen, whatever its top said (8 Nov: Egg Heist's rank screen, its garbled pills
        # taken for the party)
        if not any((t.get(f'a{i}') or '').strip() for i in range(len(self.PARTY_CARDS))):
            log.info(f"Party screen: no player's card reads, not read as a party (pills {[t.get(f'n{i}') for i in range(len(self.PARTY_SLOTS))]!r})")
            return
        # A card that read its level and CP but not its lead's name: the card sits higher on some of these screens, with the name
        # line above its box (9 Nov: the lead missing on 2 of 3 Egg Heist Party screens). That card alone is read again, taller; its
        # CP has to read the same. (The taller box on 5 such cards at 4K and 1080p: all read, and no CP changed.)
        for i, spot in enumerate(self.PARTY_CARDS):
            card = t.get(f'a{i}') or ''
            cp = re.search(r'CP\W{0,3}(\d{3,6})', card)
            if not cp or re.match(self.CARD_LEAD, card): continue
            again = self.ocr.read({'again': R.prep_grey(im, box(spot[0], spot[1] + self.PANEL_RAISED, spot[2], spot[3]), k)}).get('again') or ''
            cp2 = re.search(r'CP\W{0,3}(\d{3,6})', again)
            if re.match(self.CARD_LEAD, again) and cp2 and cp2.group(1) == cp.group(1): t[f'a{i}'] = again
        # (Season Buffs' "Lv. 4" and Challenge's "4/4" are too small and short for OCR to read reliably: left out)
        known = self.character_name; members = []
        def yellow(i):                                     # your own name is yellow (the others' white)
            x0, y0, x1, y1 = (int(v) for v in box(*self.PARTY_SLOTS[i]))
            px = [im.getpixel((x, y))[:3] for x in range(x0, x1, 3) for y in range(y0, y1, 3)]
            # (yellow from (224,192,80) to (240,224,128) by scene; the yellowish 1P badge alone is ~1% of a pill, a name 3.5-5%)
            return sum(1 for r, g, b in px if r >= 0xc8 and g >= 0xa8 and r - b >= 0x50) > 0.025 * len(px)
        for i in range(4):
            card = t.get(f'a{i}') or ''
            # Egg Heist's empty spots have no card, and their '+' and sparks read as odd names (9 Nov: 'Jm38' was sent as the party)
            if self.egg_party and not card.strip(): continue
            got = party_pill(t.get(f'n{i}') or '', i)
            if not got and self.egg_party and known and i < len(self.PARTY_SLOTS) and yellow(i): got = (i, known)   # your own pill
            if not got: continue
            slot, name = got
            pet = re.match(self.CARD_LEAD, card)
            lv = re.search(r'L\s*v\s*\.?\s*(\d{1,2})', card); cp = re.search(r'CP\W{0,3}(\d{3,6})', card)
            mine = bool(known and same_name(known, name)) or (not known and yellow(i))
            if mine and known: name = known                 # your own name is known: no misread of it on your card
            nick = pet.group(1).strip() if pet else None
            # a card read that missed (an effect over it) keeps the last read of that slot, for the same lead: by its CP,
            # which a name that reads differently each time doesn't change (8 Nov: the lead's name came through on 1 read of
            # 11, and all four cards went without it), or by the lead's name read the same again; a new CP under another
            # name is a new lead. No CP read: by the player's name
            fold = lambda v: re.sub(r'\W', '', v or '').casefold()
            old = next((m for m in (self.party or {}).get('members', []) if m['slot'] == slot and
                        ((cp and m.get('cp') == int(cp.group(1))) or (nick and fold(m.get('nickname')) == fold(nick)) or
                         (not (cp and m.get('cp')) and m['name'] == name))), {})
            members.append({'slot': slot, 'name': name, 'self': mine, 'pill': i,
                            # AI: no CP on the card (a card that didn't read falls back to the NPCs' names)
                            'ai': not mine and (not cp if card.strip() else name.casefold() in INTERLINK_NPCS),
                            # the lead's species comes from its portrait (read_party_job), never from the name on the card:
                            # players can rename their Aniimo (14 Nov, the affinity update), so that's only its nickname
                            'aniimo': old.get('aniimo'), 'nickname': nick or old.get('nickname'),
                            'aniimo_level': int(lv.group(1)) if lv else old.get('aniimo_level'),
                            'cp': int(cp.group(1)) if cp else old.get('cp')})
        boss = self.cycle_boss((t.get('boss') or '').strip(' /|'))
        row = self.stage_first(next((r for r in self.catalog if r['table'] == 'puppet_data' and r['bossTitle_en'] == boss
                                     and int(r['puppetId']) in R.active_cycle_bosses()), None) if boss else None)
        if row: self.prep_boss = (now, row)
        log.info(f"Party screen: OCR {t!r} -> boss {boss or '?'}")
        members = sorted(self.own_slots(members, now), key=lambda m: m['slot'])   # 1P..4P, one each
        self.set_party(members, now)
        if self.party: self.party['egg'] = self.egg_party
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
    # ...as the game lays the row out (UI_Node_TeamRoom_CenterItem > PetMinor1-3), in its units (TEAM_PITCH is 240 of
    # them): the portrait, 204 square, is centred 74 left of its skill badge (a round 84 across), at the same height.
    # Each tile is found by its badge (the cards don't all sit where measured), then compared over the part left of
    # the badge (x -75..11, y -52..45 around the portrait's centre), where the card's fade still shows it clearly.
    TEAM_BADGE = (74, 42)                           # the badge's centre right of the portrait's; its round's radius
    TEAM_PORTRAIT = 204
    TEAM_COMPARE = (-75, -52, 11, 45)
    # ...or, with the tile's own fade drawn in (icons.TEAM_FADE, read from the install), the whole tile left of the
    # badge: on 26 saved Party screens 299 of 312 tiles read, up from 264, and 7 wrong answers gone (6 Nov)
    TEAM_COMPARE_FADED = (-72, -56, 28, 56)
    # The card's lead Aniimo, by its head portrait: since the affinity update players can rename their Aniimo, so the name
    # on the card says nothing about the species (14 Nov). As the game lays the card out (UI_Node_TeamRoom_CenterItem >
    # PanelPetInfo > PetPrimary > Mask), in the team row's units: the portrait, 256 square, is centred 96 in from the
    # card's left and 80 down from its top, and the first team tile's 102 in and 232 down, so the lead sits 6 left of and
    # 152 above that tile. Its own mask (icons.LEAD_FADE, 256 square on the portrait) fades it into the card towards the
    # name and the card's bottom. Compared from 100 above the centre (20 over the card's top) to 90 below, and from 90
    # left to 86 right (the name starts 90 right): on the 4 saved Party screens all 16 leads read right and sure, the
    # weakest 0.60 ahead by 0.21 (the whole square, or no fade drawn in, put the weakest under tile_sure's bar)
    LEAD_OFFSET = (-6, -152)                        # the lead's centre from the first team tile's
    LEAD_PORTRAIT = 256
    LEAD_COMPARE = (-90, -100, 86, 90)
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

    def tile_centre(self, t, b, u, ux, H):
        """Where the measured layout puts a team tile's portrait centre (screen px): t is the tile's x as TEAM_WINDOW
        measures it, b the card's top."""
        sx, sy, side = self.TEAM_SQUARE
        return ux + u * (t + sx + side / 2), H * (b + sy) + u * side / 2

    def card_shift(self, ic, im, a, b, u, ux, H):
        """How far a card's team row sits from where it's measured (px), by its clearest skill badge looked for widely:
        the game sometimes draws the cards higher (5 Nov: ~40 px at 4K). (0, 0) when no badge stands out."""
        unit = self.TEAM_PITCH * u / 240; dx, r = self.TEAM_BADGE
        best = (0, 0, 0)
        for i in (1, 0, 2):                             # the middle badge, the others only when it doesn't stand out
            px, py = self.tile_centre(a + self.TEAM_PITCH * i, b, u, ux, H)
            s, x, y = ic.badge(im, px + dx * unit, py, r * unit, round(30 * unit), round(90 * unit), step=max(2, round(5 * unit)))
            if s > best[0]: best = (s, x - px - dx * unit, y - py)
            if best[0] >= 0.6: break
        return best[1:] if best[0] >= 0.4 else (0, 0)

    def team_tile(self, ic, im, t, b, u, ux, H, shift=(0, 0), memo=None):
        """One team tile's Aniimo (icons.head's answer, or None): t is the tile's x as TEAM_WINDOW measures it, b the
        card's top, shift the card's (card_shift). Its badge is looked for a few px around; with none found (a badge
        reads 0.4 or more), the tile is compared where the card's shift puts it."""
        unit = self.TEAM_PITCH * u / 240
        px, py = self.tile_centre(t, b, u, ux, H)
        px, py = px + shift[0], py + shift[1]
        dx, r = self.TEAM_BADGE
        found = ic.badge(im, px + dx * unit, py, r * unit, max(4, round(8 * unit)))
        if found[0] >= 0.4: px, py = found[1] - dx * unit, found[2]
        fade = getattr(ic, 'fade', None) is not None
        x0, y0, x1, y1 = self.TEAM_COMPARE_FADED if fade else self.TEAM_COMPARE; half = self.TEAM_PORTRAIT * unit / 2
        window = (px + x0 * unit, py + y0 * unit, px + x1 * unit, py + y1 * unit)
        return self.portrait_read(ic, im, window, (px - half, py - half, 2 * half), fade, memo)

    def lead_tile(self, ic, im, a, b, u, ux, H, shift=(0, 0), memo=None):
        """A card's lead Aniimo by its portrait (icons.head's answer, or None): a, b the card box's left and top, shift
        the card's (card_shift). Placed from the first team tile (LEAD_OFFSET), with the lead's own fade drawn in when
        the cache has it."""
        unit = self.TEAM_PITCH * u / 240
        px, py = self.tile_centre(a, b, u, ux, H)
        px, py = px + shift[0] + self.LEAD_OFFSET[0] * unit, py + shift[1] + self.LEAD_OFFSET[1] * unit
        fade = 'lead' if 'lead' in (getattr(ic, 'fades', None) or {}) else False
        x0, y0, x1, y1 = self.LEAD_COMPARE; half = self.LEAD_PORTRAIT * unit / 2
        window = (px + x0 * unit, py + y0 * unit, px + x1 * unit, py + y1 * unit)
        return self.portrait_read(ic, im, window, (px - half, py - half, 2 * half), fade, memo)

    def portrait_read(self, ic, im, window, square, fade, memo):
        """icons.head over one portrait of the Party screen, memoised by `memo` (each tile's and lead's own key)."""
        # a tile that looks as it did at the last read (same place, the picture within a few levels) keeps that read's
        # answer: between reads of one Party screen nothing moves, and matching a tile costs about half a second
        thumb = im.crop(tuple(int(v) for v in window)).convert('RGB').resize((self.TILE_THUMB, self.TILE_THUMB), Image.BILINEAR)
        store = self.__dict__.setdefault('tile_memo', {})
        last = store.get(memo) if memo is not None else None
        if last and last[0] == tuple(int(v) for v in window) and \
                max(ImageStat.Stat(ImageChops.difference(thumb, last[1])).mean) <= self.TILE_SAME:
            return last[2]
        r = ic.head(im, window, square, fade=fade)
        # only a read that counts is kept: a tile's margin moves from frame to frame around the mark (8 Nov: 0.05 to 0.21 on
        # one portrait), and a rejected read kept here stood for the same picture, its tile blank for 50 s
        if memo is not None:
            if self.tile_sure(r): store[memo] = (tuple(int(v) for v in window), thumb, r)
            else: store.pop(memo, None)
        return r

    # Your other three Aniimo stand down the fight's right side, so a slot of yours the Party screen left blank can still be
    # read there, for your own team only. Measured on 8 saved 4K fights: each portrait's whole square 190 px, centred at x 3540,
    # 217 px apart; only the top is compared (a label like "Damage Boost" lies over the bottom), left of the skill badge, which
    # dims in cooldown. Effects show through, so one frame isn't enough: HUD_AGREE frames must name the same Aniimo.
    HUD_TEAM = (0.9219, (0.3278, 0.4282, 0.5287))      # the squares' centres: x (of the UI's width), y of each row (of the height)
    HUD_SIDE = 0.0495                                  # the square's side (of the UI's width)
    HUD_WINDOW = (-0.0083, -0.0338, 0.0117, 0.0032)    # what's compared, from the centre: x0, y0, x1 (of the width), y1 (of the height)
    HUD_EVERY_S = 4.0
    HUD_AGREE = 2
    hud_at = 0; hud_job = None

    def fight_team(self, im, now, u, ux, H):
        """A look at the fight's right side, in the background, while your own team has a blank slot."""
        party = self.party
        if not party or self.icons_state != 'ready' or now - self.hud_at < self.HUD_EVERY_S: return
        if self.hud_job and self.hud_job.is_alive(): return
        mine = next((m for m in party['members'] if m['self']), None)
        if not mine: return
        team = (party.get('teams') or [None] * 4)[mine['slot']]
        if team and all(team): return
        self.hud_at = now
        self.hud_job = threading.Thread(target=self.read_fight_team, args=(im, party, mine, u, ux, H), daemon=True); self.hud_job.start()

    def read_fight_team(self, im, party, mine, u, ux, H):
        try:
            teams = party.setdefault('teams', [None] * 4); slot = mine['slot']
            team = list(teams[slot] or [None] * 3); votes = party.setdefault('hud_votes', {})
            side = self.HUD_SIDE * u; x = ux + u * self.HUD_TEAM[0]; x0, y0, x1, y1 = self.HUD_WINDOW
            for i, row in enumerate(self.HUD_TEAM[1]):
                if team[i]: continue
                y = H * row
                r = self.icons.head(im, (x + u * x0, y + H * y0, x + u * x1, y + H * y1), (x - side / 2, y - side / 2, side))
                # a team has no Aniimo twice, and your lead (its portrait's species) isn't one of the three
                if not self.tile_sure(r) or r[0] in team or r[0] == mine.get('aniimo'): continue
                n = votes[(i, r[0])] = votes.get((i, r[0]), 0) + 1
                if n < self.HUD_AGREE: continue
                team[i] = r[0]; teams[slot] = team; party.setdefault('team_who', {})[slot] = self.who(mine); party['team'] = team
                log.info(f'Your team, slot {i + 1}: {r[0]}, from the fight (blank on the Party screen; fit {r[1]:.2f}, ahead by {r[2]:.2f})')
        except Exception: log.exception('Reading your team in the fight failed')

    @staticmethod
    def tile_sure(r):
        """Whether a portrait match (icons.head's answer) counts: clearly ahead of the next Aniimo."""
        return bool(r and ((r[1] >= 0.55 and r[2] >= 0.15) or (r[1] >= 0.68 and r[2] >= 0.10)))

    def read_party_icons(self, im, cards, u, ux, H, now):
        """Each player's team (cards: (slot, pill) per player) and the leader's buffs from the Party screen's pictures, matched to the game's own (icons.py), in
        the background (a few seconds a read). The newest read of a tile stands (a tile it couldn't read keeps the last
        one), so a team changed before the fight shows from the next read; a player whose name or lead Aniimo changed
        starts over (7 Nov: a 2-of-3 vote kept the last run's teams for runs read only once or twice, 13 tiles in 141
        sent wrong). A frame that comes while a read runs is read right after it (the last one before the fight
        counts most). Nothing is read until the icons are loaded."""
        self.load_icons()
        if self.icons_state != 'ready' or not self.party: return
        job = (im, cards, u, ux, H, {m['slot']: self.who(m) for m in self.party['members']})
        if self.icon_read and self.icon_read.is_alive():
            self.icon_next = job; return
        def run(job):
            while job:
                self.icon_next = None
                try: self.read_party_job(*job)
                except Exception: log.exception('Party screen icons: read failed')
                job = self.icon_next
        self.icon_read = threading.Thread(target=run, args=(job,), daemon=True); self.icon_read.start()

    icon_next = None
    TILE_THUMB = 24       # a tile's picture shrunk to this many px a side, to tell whether it changed
    TILE_SAME = 2.0       # ...and each colour's mean difference at most this many levels: the same picture

    @staticmethod
    def who(m):
        """Who a Party screen card is, as far as its team goes: the player's name and their lead's species, from its
        portrait (casefolded). Not the name on the card: a renamed or misread lead is the same lead."""
        return tuple(re.sub(r'\W', '', str(m.get(k) or '')).casefold() for k in ('name', 'aniimo'))

    def read_party_job(self, im, cards, u, ux, H, who):
        """read_party_icons' read of one frame; who: each slot's who() when the frame came."""
        box = lambda x0, y0, x1, y1: (ux + u * x0, H * y0, ux + u * x1, H * y1)
        ic, teams, leads, shifts = self.icons, {}, {}, []
        def read_card(slot, card):                      # every player's card has the same team row
            a, b = self.PARTY_CARDS[card][:2]; team = []
            shift = self.card_shift(ic, im, a, b, u, ux, H)     # each read: the cards move when all are ready
            for i in range(3):
                r = self.team_tile(ic, im, a + self.TEAM_PITCH * i, b, u, ux, H, shift, memo=(card, i))
                # accepted only when clearly ahead (checked on 312 tiles: no wrong answer passes); an Aniimo whose
                # portrait isn't cached scores low against everything and stays unread (Leafy, 30 Oct, until
                # icons.py read the patch sets' heads), and so does a tile under something drawn over the card
                team.append(r[0] if self.tile_sure(r) else None)
            # the lead by its portrait, on the same bar as the tiles (unsure: None, and the last sure read stands)
            r = self.lead_tile(ic, im, a, b, u, ux, H, shift, memo=(card, 'lead'))
            return team, shift, r[0] if self.tile_sure(r) else None
        # (one card after another: reading them on threads side by side was no faster, the matching being Python, 8 Nov)
        for slot, card in cards:
            teams[slot], shift, leads[slot] = read_card(slot, card); shifts.append(shift)
        # the buffs panel moves when the whole screen is drawn higher (at times ~40 px at 4K: 6 of 39 saved screens,
        # none of them read with its Special Offering): about 1.9 times the cards' shift, as it sits lower; the panel's
        # offset is picked by the first Season Buff's fit (always there). Only a clear shift of every card counts: the
        # few px each card's fit wanders don't (the buff squares mind even 3 px)
        # A panel that doesn't fit where expected is looked for a little around it (3 saved screens had it 0.5-1% of
        # the height lower, the cards where measured).
        if self.egg_party: return self.party_teams(teams, who, leads)    # no buffs on this screen
        dys = sorted(s[1] for s in shifts); dy = dys[len(dys) // 2] if dys else 0
        at = lambda off: (lambda x0, y0, x1, y1: (ux + u * x0, H * y0 + off, ux + u * x1, H * y1 + off))
        fits = {}
        def fit(off):
            if off not in fits: fits[off] = (ic.buff(im, at(off)(*self.SEASON_BUFFS[0]), 'season') or (None, -1))[1]
            return fits[off]
        sy = 0
        if abs(dy) > 10 * H / 2160 and all(abs(d - dy) < 6 * H / 2160 for d in dys):
            sy = max((round(dy * f) for f in (1.7, 1.8, 1.9, 2.0, 2.1)), key=fit)
        if fit(sy) < 0.8:
            sy = max([sy, round(H * self.PANEL_RAISED)] + [sy + round(H * k * 0.005) for k in (1, 2, 3, -1, -2, -3)], key=fit)
        box = lambda x0, y0, x1, y1: (ux + u * x0, H * y0 + sy, ux + u * x1, H * y1 + sy)
        season = []
        for sq in self.SEASON_BUFFS:
            r = ic.buff(im, box(*sq), 'season')
            if not r or r[1] < 0.8 or r[2] < 0.2: season.append(None); continue
            x0, y0, x1, y1 = self.SEASON_LEVEL
            d = ic.digit(im, box(sq[0] + x0, y0, sq[0] + x1, y1))
            level = int(d[0]) if d and d[2] >= 0.055 else None     # (Lv. 6 is the closest call: 0.061-0.065 on real screens, every other digit 0.13 up)
            ids = ic.buffs_named(r[0], 'season', level) if level else []
            names = [ic.buffs[k]['name'] for k in ic.buffs_named(r[0], 'season')]
            season.append({'id': int(ids[0]), 'name': ic.buffs[ids[0]]['name'], 'level': level} if ids else
                          {'name': max(set(names), key=names.count)})
        self.panel_sy = sy
        # (the Special Offering: party_circle's, as soon as it changes; this read is older by the time it's done)
        offering = None if self.circle_on else self.read_offering(ic, im, box(*self.OFFERING))
        party = self.party_teams(teams, who, leads)
        if not party: return
        out = party.get('teams')
        # each buff square keeps its last read when this one couldn't make it out (a read that caught the Season Buffs
        # but not the Special Offering used to drop the offering)
        with self.buff_lock:
            last = party.get('buff_reads') or {'season': [None] * len(season), 'offering': None}
            # no Season Buff at all on a panel at rest: padlocks, someone else's party. An earlier party's aren't this one's
            # (8 Nov: a run went with the player's own lobby's)
            kept = [None] * len(season) if not any(season) and self.panel_at_rest(im, u, ux, H) else last['season']
            self.set_buffs(party, [n or o for n, o in zip(season, kept)], offering or last['offering'])
        log.info(f"Party screen icons: leads {[m.get('aniimo') for m in party['members']]}, teams {out}, buffs {party.get('buffs')}")

    def party_teams(self, teams, who, leads=None):
        """Each card's team reads into the party (by slot, 1P..4P), the newest of each tile standing, and its lead's
        species from its portrait (leads: slot -> species or None; a read that wasn't sure keeps the last one)."""
        party = self.party
        if not party: return None
        members = {m['slot']: m for m in party['members']}
        now_who = {s: self.who(m) for s, m in members.items()}
        out = party.setdefault('teams', [None] * 4)
        had = party.setdefault('team_who', {})          # whose team each slot's reads are
        for slot, team in teams.items():
            # the card changed hands while it was read (or its lead: a new CP clears the species). A species learnt by
            # the read before this one, which came while this frame waited, is the same card
            then, now = who.get(slot), now_who.get(slot)
            if now != then and not (then and now and then[0] == now[0] and not then[1]): continue
            m, lead = members.get(slot), (leads or {}).get(slot)
            # a lead read for the first time keeps the team read so far; another species than the one known is another
            # lead, and its team starts over
            other = bool(m is not None and lead and m.get('aniimo') and lead != m['aniimo'])
            if m is not None and lead: m['aniimo'] = lead
            last = out[slot] if not other and had.get(slot) == now and out[slot] else [None] * 3
            out[slot] = [n or o for n, o in zip(team, last)]
            had[slot] = self.who(m) if m is not None else who[slot]
        mine = next((m['slot'] for m in party['members'] if m['self']), None)
        if mine is not None: party['team'] = out[mine]
        if self.egg_party: log.info(f'Egg Heist party: teams {out}')
        return party

    @staticmethod
    def set_buffs(party, season, offering):
        party['buff_reads'] = {'season': season, 'offering': offering}
        party['buffs'] = [b for b in season if b] + ([offering] if offering else [])

    @staticmethod
    def read_offering(ic, im, box):
        """The Special Offering in its circle, or None. None chosen yet shows '+' or '...' (up to 0.61 against the icons);
        one chosen read 0.78 (5 Nov, Elemental Concerto)."""
        r = ic.buff(im, box, 'offering', text_below=False)
        if not (r and r[1] >= 0.7 and r[2] >= 0.15): return None
        ids = ic.buffs_named(r[0], 'offering')            # a few offerings share an icon: then only their names go
        names = sorted({ic.buffs[k]['name'] for k in ids})
        return {'id': int(ids[0]), 'name': names[0]} if len(names) == 1 else {'name': ' or '.join(names)}

    # The Special Offering can change until the fight starts, and a whole read of the screen takes seconds (8 Nov: the last
    # one stayed 3 s after the change). So its circle alone is looked at twice a second: the same picture costs nothing, and
    # a new one is read as soon as it holds still, and what was there before goes. That's only while the panel's at rest: a
    # frame without its light panel is the screen fading into the fight, whose last read stands. (Not by the Season Buffs: in
    # someone else's party those squares are padlocks, 8 Nov, and no offering was read at all.)
    circle_on = False; circle_at = 0; circle_prev = None; circle_read = None; circle_tried = 0; circle_job = None; panel_sy = 0
    buff_lock = threading.Lock()
    CIRCLE_EVERY_S = 0.5
    CIRCLE_RETRY_S = 3.0      # a picture that doesn't read ('+' before one is chosen): not again and again

    def party_circle(self, im, now, u, ux, H):
        if not self.party or self.egg_party or self.icons_state != 'ready' or now - self.circle_at < self.CIRCLE_EVERY_S: return
        self.circle_on = True; self.circle_at = now
        x0, y0, x1, y1 = self.OFFERING
        box = (ux + u * x0, H * y0 + self.panel_sy, ux + u * x1, H * y1 + self.panel_sy)
        thumb = im.crop(tuple(int(v) for v in box)).convert('RGB').resize((self.TILE_THUMB, self.TILE_THUMB), Image.BILINEAR)
        same = lambda a: a is not None and a.size == thumb.size and max(ImageStat.Stat(ImageChops.difference(thumb, a)).mean) <= self.TILE_SAME
        prev, self.circle_prev = self.circle_prev, thumb
        if not same(prev): return                                  # still moving
        known = (self.party.get('buff_reads') or {}).get('offering')
        if same(self.circle_read) and (known or now - self.circle_tried < self.CIRCLE_RETRY_S): return
        if self.circle_job and self.circle_job.is_alive(): return
        self.circle_tried = now
        self.circle_job = threading.Thread(target=self.read_circle, args=(im, box, thumb, u, ux, H), daemon=True); self.circle_job.start()

    PANEL_RAISED = -0.0352                       # where the panel goes when everyone's ready, of the height
    PANEL_SPOT = (0.405, 0.885, 0.43, 0.90)     # the panel's own light blue, left of the circle
    PANEL_BLUE = 170                             # at least this much blue: 183-220 on 24 saved Party screens, 167 at 90% of a fade

    def panel_at_rest(self, im, u, ux, H):
        x0, y0, x1, y1 = self.PANEL_SPOT
        spot = im.crop((int(ux + u * x0), int(H * y0 + self.panel_sy), int(ux + u * x1), int(H * y1 + self.panel_sy)))
        return ImageStat.Stat(spot.convert('RGB')).mean[2] >= self.PANEL_BLUE

    def read_circle(self, im, box, thumb, u, ux, H):
        try:
            ic = self.icons
            if not self.panel_at_rest(im, u, ux, H): return
            offering = self.read_offering(ic, im, box); party = self.party
            if offering is None:                 # the panel moves when everyone's ready (8 Nov: ~76 px up, and the offering went blank)
                x0, y0, x1, y1 = self.OFFERING
                for sy in {0, round(H * self.PANEL_RAISED)} - {self.panel_sy}:
                    offering = self.read_offering(ic, im, (ux + u * x0, H * y0 + sy, ux + u * x1, H * y1 + sy))
                    if offering: self.panel_sy = sy; break
            if not party: return
            with self.buff_lock:
                last = party.get('buff_reads') or {'season': [None] * len(self.SEASON_BUFFS), 'offering': None}
                self.set_buffs(party, last['season'], offering)
            self.circle_read = thumb
            if offering != last['offering']: log.info(f"Special Offering: {(offering or {}).get('name')} (was {(last['offering'] or {}).get('name')})")
        except Exception: log.exception('Party screen: reading the Special Offering failed')

    def set_party(self, members, now):
        self.screen_settled('party', now)
        """This run's party (from the old prep screen or the Party screen), each slot's name voted over a few reads."""
        if not members: return
        # a read that missed a player (the screen fading into the fight, an effect over a name) keeps them: a player
        # is dropped only when two reads in a row miss them (6 Nov: the last read before the fight had 3 of 4 names,
        # and the run went with 3)
        self.missed_now = {}
        if self.party and now - self.party['at'] < 60:
            have = {m['slot']: m for m in self.party['members']}
            got = {m['slot'] for m in members}
            missed = self.party.get('missed', {})
            keep = [dict(m, kept=True) for s, m in have.items() if s not in got and not missed.get(s)]
            if keep: members = sorted(members + keep, key=lambda m: m['slot'])
            self.missed_now = {s: 1 for s in have if s not in got}
        # the same party is read every few seconds: keep each slot's most common reading (OCR slips now and then)
        same = self.party and now - self.party['at'] < 900 and \
            [m['slot'] for m in self.party['members']] == [m['slot'] for m in members]
        votes = self.party['votes'] if same else {}
        for m in members: votes.setdefault(m['slot'], collections.Counter())[m['name']] += 1
        for m in members: m['name'] = votes[m['slot']].most_common(1)[0][0]
        if same:                                        # your name misread once ('IMInaX') is still you
            was = {m['slot']: m for m in self.party['members']}
            for m in members:
                if not m['self'] and was.get(m['slot'], {}).get('self') and was[m['slot']]['name'] == m['name']:
                    m['self'] = True; m['ai'] = False
        # AI: a sign read on two reads, never on one. The Party screen's AI is a card with no CP read (or the old prep
        # screen's 'Ai' badge), and a CP missed on one read (an effect over the card) marked a real player AI for the run
        # (14 Nov: a human teammate went as AI). So a slot's player is AI once two reads say so and none read their CP
        # (a CP is only ever on a real player's card); one of the game's own AI teammates by name (INTERLINK_NPCS) at once
        recent = self.party and now - self.party['at'] < 900
        signs = {k: list(v) for k, v in self.party.get('ai_signs', {}).items()} if recent else {}
        for m in members:
            if m.get('kept'): continue                  # a slot this read missed keeps what it had
            key = (m['slot'], TEAM.fold(m['name']))
            tally = signs.setdefault(key, [0, 0])       # reads that said AI, reads that said a real player
            tally[0 if m['ai'] else 1] += 1
            m['ai'] = not m['self'] and ((tally[0] >= 2 and tally[1] == 0) or (m['ai'] and m['name'].casefold() in INTERLINK_NPCS))
        # One player in one slot only: the same name in two slots is one read's badge misread (a '2P' as '3P') next to the
        # slot kept from a read before (14 Nov: one teammate in slots 2 and 3, the run sent with them twice). The slot whose
        # pill sits in its own place keeps the name (then you, a slot read this time, the commoner read); the other copy
        # goes when it was only kept, else its name is left blank: a blank is better than the same player twice
        groups = {}
        for m in members:
            if m['name']: groups.setdefault(TEAM.fold(m['name']), []).append(m)
        drop = []
        for same_name_ in (g for g in groups.values() if len(g) > 1):
            best = max(same_name_, key=lambda m: (m.get('pill', m['slot']) == m['slot'], bool(m['self']), not m.get('kept'),
                                                  votes.get(m['slot'], {}).get(m['name'], 0)))
            for m in same_name_:
                if m is best: continue
                log.info(f"party: {m['name']!r} in slot {m['slot'] + 1} too, kept in slot {best['slot'] + 1}: slot {m['slot'] + 1} left {'out' if m.get('kept') else 'blank'}")
                if m.get('kept'): drop.append(m)
                else: m.update(name='', ai=False, self=False)
        members = [m for m in members if not any(m is d for d in drop)]
        for m in members: m.pop('kept', None)
        counted = self.party.get('counted', set()) if same else set()
        for m in members:
            if m['self'] or m['ai'] or not m['name']: continue
            clear = votes[m['slot']][m['name']] >= 3
            m['name'] = TEAM.steady(m['name'])
            if clear and m['slot'] not in counted: TEAM.saw(m['name']); counted.add(m['slot'])
        changed = not same or [(m['name'], m['ai'], m['self']) for m in members] != \
            [(m['name'], m['ai'], m['self']) for m in self.party['members']]
        # the buffs are the leader's (1P): they carry on while 1P is the same player, even when another pill misread
        lead = lambda ms: next((self.who(m)[0] for m in ms if m['slot'] == 0), None)
        keep_buffs = self.party and now - self.party['at'] < 900 and lead(members) and lead(members) == lead(self.party['members'])
        kept = {k: self.party[k] for k in ('buffs', 'buff_reads') if k in self.party} if same or keep_buffs else {}
        # each card's team reads carry on while it's the same player with the same lead Aniimo (even when another
        # slot's pill misread for a moment); a card that changed hands or lead starts over (no last run's team)
        if self.party and now - self.party['at'] < 900:
            who = {m['slot']: self.who(m) for m in members}
            had = {s: w for s, w in self.party.get('team_who', {}).items() if who.get(s) == w}
            old = self.party.get('teams') or [None] * 4
            kept['teams'] = [old[s] if s in had and s < len(old) else None for s in range(4)]
            kept['team_who'] = had
            mine = next((m['slot'] for m in members if m['self']), None)
            if mine is not None and kept['teams'][mine]: kept['team'] = kept['teams'][mine]
        self.party = {'at': now, 'members': members, 'votes': votes, 'counted': counted, **kept,   # the icon reads carry on too
                      'missed': getattr(self, 'missed_now', {}), 'ai_signs': signs}
        if changed:
            log.info('party: ' + ', '.join(f"{m['name']}{' (AI)' if m['ai'] else ''}{' (you)' if m['self'] else ''}"
                                           + (f" [{m.get('aniimo') or '?'}{' as ' + repr(m['nickname']) if m.get('nickname') and m['nickname'] != m.get('aniimo') else ''}"
                                              f" Lv {m.get('aniimo_level')}{', CP ' + str(m['cp']) if m.get('cp') else ''}]"
                                              if m.get('aniimo') or m.get('nickname') else '')
                                           for m in members))

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
        # a Korean name (the Korean model read it and the game's Korean letters fit) stays: the short-name matcher knows
        # only the game's Latin letters, and turned a player's two-letter Korean name into Latin look-alikes (8 Nov)
        if where == 'menu' and self.uid and self.uid in digits and len(name) <= 2 and not K.has_hangul(name) \
                and im is not None and u:
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

    # ---- Link character (the app's button): read the Profile screen, then send the name once the player confirms.
    # The app writes character_link.json - {"request": time}, then {"request": time, "confirm": {"uid", "name"}} - and
    # deletes it to cancel. What the meter sees goes in status.json's character.link: waiting / found / done / timeout.
    LINK_FILE = 'character_link.json'
    LINK_SECONDS = 120          # how long a request waits for the Profile screen
    LINK_READS = 3              # Profile reads in a row that must agree

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
        reader = self.name_reader() if mask is not None and len(name) <= 3 and not K.has_hangul(name) else None
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

    LEADER_NOTE = (0.33, 0.140, 0.67, 0.195)          # "The team leader has ended the challenge.", top center

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
        # (Dev switch proof-off-thread: encoded by the picture writer; the top-only canvas is painted over on the next look)
        pic = im.copy() if im is getattr(self, 'canvas', None) and D.on('proof-off-thread') else im
        folder, name = os.path.join('end_screens', a[0]), time.strftime('%H%M%S')
        picture_job(lambda: save_picture(pic, folder, name))

    BAR_LOST_S, BAR_LOST_EACH, BAR_LOST_MAX = 60, 12, 60   # after a lost boss bar: this long, this many, this many a session

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

    def step(self):
        self.reload_settings(time.time())
        # a screen read whose players' names are in now (names_first): handled before anything else, as when the names
        # were read in place, and the frame loop stood still meanwhile
        self.names_due()
        # holds end on time even without a picture (the game closed or minimized during one): checked first
        hold = getattr(self, 'egg_hold', None)
        if hold and time.time() > hold[0]: self.egg_hold_end('time limit')
        self.auto_active()
        # Each plug-in has its own switch. The DPS meter off (its switch in the app, or the button over the game) hides
        # everything of the meter; the game is still read while the leaderboards are on (their own switch), since that's
        # where the records come from: runs, end screens, Battle Data and proofs, with nothing of the meter over the
        # game (quiet). Both off: end what's open and rest until one is back on.
        meter_off = self.args.replay is None and wc.shared.switched_off()
        self.switches.tick(self.args.cfg, not meter_off)     # a stopped plug-in switched off and on: another go
        mode = meter_mode(meter_off, self.args.cfg)
        rest, quiet = mode == 'rest', mode == 'quiet'
        if quiet != getattr(self, 'quiet', False):
            self.quiet = quiet
            log.info('the DPS meter is off: reading results for the leaderboards only' if quiet else 'the DPS meter shows again')
        if rest != getattr(self, 'capture_off', False):
            self.capture_off = rest
            log.info('the DPS meter and the leaderboards are off: resting' if rest else 'reading the game again')
            if rest:
                self.end_auto('the DPS meter and the leaderboards were switched off')
                if getattr(self, 'egg_hold', None): self.egg_hold_end('the DPS meter and the leaderboards were switched off')
                self.hold.stop()
                if CT.hidden: CT.hide_controllers(False)
                if self.fight: self.end_fight('the DPS meter was switched off')
                self.bar = None
        if rest:
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
        if getattr(self, 'sim_tour', None): partial = top_only = False; due = False      # the Gizmo tour reads whole frames
        im = self.grab(partial, top_only, self.screen_parts(time.time()) if due == 'screen' else None)
        self.away_note(im)
        if not getattr(self, 'quiet', False): self.hit_watch(time.time())   # "You're being attacked": the DPS meter's
        if im is None:
            self.out('status', 'waiting for the game window' if not self.hwnd else "can't see the game window"); return
        now = time.time()
        if self.fight and self.hwnd and self.args.replay is None and not wc.is_foreground(self.hwnd):
            self.fight.away += min(1.0, now - getattr(self, 'last_step', now))   # time the fight ran while you were tabbed out
        self.last_step = now
        top_h = max(1, int(self.frame_size[1] * TOP_PART))
        self.top_sig = self.top_sig_fast(im, top_h, now)
        if not self.fight and self.bar is None and D.on('busy-half-pace'): self.calm_watch(self.top_sig, now)
        if self.frame_full: self.egg_fade(im, now)        # Egg Heist: hold inputs through the fade to its end screen
        self.sim_check(now)
        if self.holosim_space and self.args.replay is None: self.sim_test_key(now)    # Ctrl+Shift+F11: test the Sim end
        # the Gizmo tour first, before any bar search: its list's lines and the Core's gauge looked like boss bars, and the
        # tour waited 10 s for each to be thrown out (14 Nov: 22 s before its first read)
        if getattr(self, 'sim_tour', None) and self.frame_full and self.sim_tour_step(im, now): self.snapshot(now); return
        if self.bar is None and self.holosim_space: self.sim_gate(im, now)    # Holo-Battle Sim's exit gate (last level)
        # -- keep the bar locked: re-check its brackets, search again only when they are gone
        if self.bar is None or not R.verify_bar(im, self.bar):
            if partial:                                   # searching and end screens need the whole frame
                im = self.grab()
                if im is None: return
            # the spot the bar had last time first (~1 ms), then the whole-frame search (~3 ms): both every frame
            found = R.bar_at(im, self.bar_spots) or (None if self.search_rests(now) else R.find_bar(im))
            if found and not self.fight and self.is_dud(im, found): found = None
            if found and not self.plausible_bar(found, im): found = None
            if found and found.get('world') and ((self.fight and self.fight.bossrush) or self.bossrush_space):
                found = R.as_bossrush_bar(found)                    # Interlink at x1: the xN box can be gone
            elif found and not self.bossrush_space and scene_of(self.space) is not None and self.args.replay is None:
                found = R.as_world_bar(found)                       # the open world: a hit flash is no xN box
            # the DPS meter off, only the leaderboards reading (quiet): a boss out in the world is the meter's alone (no
            # record comes of it), so it isn't followed - Interlink, Holo-Battle Sim and an open fight still are
            if (found and getattr(self, 'quiet', False) and found.get('world') and not self.fight
                    and not (self.bossrush_space or self.holosim_space)):
                found = None
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
        box = im.crop(tuple(int(v) for v in xn_box))
        # digit shapes, every frame: the last answer again while the box's white shapes (all read_counter looks at) are
        # the very same as in the last read
        xn = xn_digits = X.read_counter_same(box, self.xn_tpl, top)
        if not self.fight: self.instant_start(xn, now, low=bar['yc'] > TOP_BAR * self.frame_size[1])
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

    def plugin_status(self):
        """Each plug-in this process runs (plugins.json, those that need the game read), for the app's sidebar: its
        switch, its state ('on', 'off', 'resting', 'waiting', 'attention', 'failed') and why, and its last error."""
        cfg = self.args.cfg
        meter_on = not (self.args.replay is None and wc.shared.switched_off())
        out = {}
        for pid, p in PLUGINS.items():
            if 'reader' not in p['needs']: continue
            health = HEALTH.status(pid); on = switched_on(cfg, pid, meter_on)
            if health['failed']: state, why = 'failed', 'stopped after repeated errors: turn it off and on to try again'
            elif not on: state, why = 'off', ''
            elif getattr(self, 'homeland_rest', False): state, why = 'resting', 'in a homeland: nothing is read there'
            elif not self.hwnd: state, why = 'waiting', 'waiting for the game'
            elif pid == 'leaderboards' and not str((cfg.get('leaderboard') or {}).get('token', '')).startswith('alb_'):
                state, why = 'attention', 'not signed in: sign in on its Account tab'
            else: state, why = 'on', ''
            out[pid] = {'on': on, 'state': state, 'why': why, 'last_error': health['last_error']}
        return out

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
            'plugins': self.plugin_status(),                            # each plug-in this process runs (the sidebar)
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
            # the game's icons from the start: rebuilt after a game patch or an aLogs update they take ~40 s, and
            # loaded only at the Party screen they were ready after it was gone (6 Nov: a run sent without teams)
            self.load_icons()
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
            # Dev switch busy-half-pace: 10 looks a second while busy_calm, except in Interlink rooms, whose boss bars
            # start timed fights (the screen reads slow down there too: screen_read_due)
            elif D.on('busy-half-pace') and self.busy_calm(time.time()): interval = self.calm_pace(interval)
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
        self.end_fight('meter closed')
        self.send_waiting_run()                                      # a run waiting for its Battle Data isn't lost
        self.ocr.close()
        if WRITER.q.unfinished_tasks: WRITER.flush(3.0)              # proof-off-thread: the pictures still waiting
        if self.submitter and getattr(self, 'presence_sent', None) and self.presence_sent[1] != 'offline':
            self.submitter.presence(self.presence_sent[0], 'offline', wait=True)     # aLogs closing: you're offline
        if self.ocr_bg: self.ocr_bg.close()
        if self.screen_bg: self.screen_bg.close()
        if self.names_bg: self.names_bg.close()
        if self.submitter: self.submitter.stop = True
        if self.logw: self.logw.stop = True

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
