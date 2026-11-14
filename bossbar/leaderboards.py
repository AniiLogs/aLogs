"""The leaderboards plug-in: deciding which runs go to the AniiLogs leaderboards and sending them (submit.py), the
proof automation (Battle Data's button, Holo-Battle Sim's exit gate and Gizmo tour, Egg Heist's fade hold), linking
your character to your account, and your online status. Its own switch: leaderboard.submit."""
from core import *
from results import *
import logging
import devflags as D
import shutil

LATE_BATTLE_DATA_S = 180            # after a run went without Battle Data: how long a Battle Data opened later still follows it

log = logging.getLogger('meter.leaderboards')   # its lines in meter.log are tagged [leaderboards]


EGG_BLACK = 0.12        # s of black screen that count as Egg Heist's fade to its end screen (egg_fade)
RESULT_PAUSE = 5                    # seconds the automatic Battle Data session stays on the result screen before clicking,
SIM_GATE_GONE = 3           # s with the exit gate's button gone before a press no longer counts as ending the run
SIM_PRESS_EARLY = 1.0       # s before the exit gate's button was first seen that a press still counts (it shows first)
SIM_GATE_PRESSED = 0.5      # s with the exit gate's button gone (after it was up) that count as your press of it
SIM_VICTORY_WAIT = 8        # s after your press at the exit gate for the Victory screen (it comes in ~1 s) before control comes back
HOLD_MIN = RESULT_PAUSE + BATTLE_DATA_PAUSE   # "Block for at most" never shorter than both pauses (10 s)

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
    dark armor beside the button. The white letters' rows across the middle have too little of it and are bridged
    (up to 0.008 of the width): 13 Nov, at 4K on a brown ground, they split the pill into two bands of about 0.008 each,
    and with the band counted only unbroken the button was never seen."""
    band = im.crop(tuple(int(v) for v in box)).convert('RGB')
    w = 400; h = max(1, round(band.height * w / band.width)); band = band.resize((w, h), Image.BOX)
    scale = w / (box[2] - box[0])
    need = 0.04 * u * scale; px = band.load(); rows = 0; gap = 0; best = 0   # (lettered rows: 0.053+)
    for y in range(h):
        n = 0
        for x in range(w):
            r, g, b = px[x, y]
            if 55 <= r <= 100 and 55 <= g <= 100 and b <= 125 and b - r >= 8 and b - g >= 8: n += 1
        if n >= need: rows += 1; gap = 0
        else:
            gap += 1
            if gap > 0.008 * u * scale: rows = 0
        best = max(best, rows)
    return best >= 0.012 * u * scale


class LeaderboardsMixin:
    """The leaderboards plug-in's part of the Meter (live.py): its methods, sharing the Meter's state."""

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
        # (No hold starts on a fade any more: it took black loads inside a run for the end, holding keys in play, 9 Nov: 3 times in
        # one match. The end and rank screens hold when they're spotted: egg_screen_hold.)
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

    # Any mode's captured result goes through send_result: inputs stay held until it's sent and its notice is up, so a press can't
    # skip past anything meanwhile (the owner, 8 Nov: the default for every mode, present and future). SEND_HOLD_S is the failsafe.
    SEND_HOLD_S = 10
    send_waiting = None

    def send_result(self, payload, evidence, **files):
        self.submitter.enqueue(payload, evidence, **files)
        r = self.args.cfg.get('results') or {}
        if self.args.replay is not None or not C.automation(self.args.cfg) or not r.get('hold_inputs', True): return
        if self.hwnd and not wc.is_foreground(self.hwnd): return
        self.send_waiting = payload.get('clientRunId')
        self.hold.start(self.SEND_HOLD_S)
        log.info(f'holding inputs until this is sent (at most {self.SEND_HOLD_S} s)')

    def result_sent(self, payload):
        """From the sender, once a result is sent: its notice is going up, so inputs come back."""
        if self.send_waiting and payload.get('clientRunId') == self.send_waiting:
            self.send_waiting = None; self.hold.stop()

    def egg_hold_end(self, why):
        if not getattr(self, 'egg_hold', None): return
        self.egg_hold = None; self.hold.stop()
        if CT.hidden: CT.hide_controllers(False)
        log.info(f'Egg Heist input hold ended: {why}')

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
            self.send_result(S.sim_payload(sim, self.uid), os.path.relpath(shot, HERE) if shot else None, gizmos=shots)
        self.screen_settled('sim_result', time.time())
        self.out('status', f"Holo-Battle Sim recorded: {sim['trial']} - {sim['difficulty']} in {sim['time_s']} s"
                 + (f" ({note})" if note else ''))
        self.sim_release('the run is saved')

    def sim_control(self, now, why):
        """aLogs takes control for the Holo-Battle Sim's end (Leaderboards › Automation): your keyboard and mouse
        buttons held (and the controller hidden) as set, for at most "Take control for at most" seconds in all, from
        the exit gate (or the Victory screen) to the Gizmo screen read. Kept when it's already on."""
        ctl = getattr(self, 'sim_ctl', None)
        if ctl: return ctl
        r = self.args.cfg['results']
        seconds = max(HOLD_MIN, min(120, int(r.get('hold_seconds') or 60)))
        self.sim_ctl = ctl = {'until': now + seconds, 'why': why, 'held': bool(r.get('hold_inputs')), 'space': self.space}
        if ctl['held']:
            self.hold.start(seconds)
            # the controller too: the game in the background reads none (HidHide's hide can't reach a controller the
            # game already has open; 11 Nov, a Sim end skipped with it)
            if self.hwnd and self.args.replay is None: self.focus_guard.take(self.hwnd, seconds)
        if r.get('hold_controller') and CT.available(): CT.hide_controllers(True)
        log.info(f'Holo-Battle Sim: aLogs has control ({why}), for at most {seconds} s')
        return ctl

    def sim_release(self, why):
        if not getattr(self, 'sim_ctl', None): return
        self.sim_ctl = None; self.hold.stop(); self.focus_guard.stop()
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
            gate['up_at'] = now; gate['gone'] = None
        elif gate.get('first_up'):
            # the button gone after it was up: you pressed it (the owner, 13 Nov: take control as soon as it goes away;
            # a press the watch missed counts too). Gone for SIM_GATE_PRESSED, so one look that misses it isn't one.
            # Walking away hides it too: then no Victory screen comes and control comes back after SIM_VICTORY_WAIT.
            gate['gone'] = gate.get('gone') or now
            if now - gate['gone'] >= SIM_GATE_PRESSED:
                watch.stop(); gate['used'] = now
                ctl = self.sim_control(now, 'the exit gate button went away (your press)')
                ctl.update(stage='gate', done=now); gate['first_up'] = gate['up_at'] = gate['gone'] = None
                log.info('Holo-Battle Sim: waiting for the Victory screen')

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
        if ctl.get('space') and self.space != ctl['space']:    # the game left the Sim: no more clicks into the next place
            self.sim_tour_end('the game left the Victory screen'); return False
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
        if self.hold.active: self.hold.freeze_moves(0.4)          # the player's mouse can't pull the click away
        wc.click_at(x, y)

    @staticmethod
    def run_path(f):
        """The fight's logs/fights/<fight>.json."""
        return os.path.join(HERE, 'logs', 'fights', os.path.splitext(os.path.basename(f.log.name))[0] + '.json')

    def run_to_send(self, f):
        """Is this fight's run waiting for its Battle Data, going to the leaderboard and not sent yet, or sent without
        Battle Data inside its late window (LATE_BATTLE_DATA_S)? (Then its Battle Data screenshot is kept as proof.)"""
        try:
            with open(self.run_path(f), encoding='utf-8') as fh: s = json.load(fh)
        except (OSError, ValueError): return False
        return (s.get('submit', True) is not False and not s.get('sent')) or self.late_open(s)

    @staticmethod
    def late_open(s):
        """A sent run whose Battle Data can still follow it (send_late_battle_data)."""
        return bool(s.get('sent') and s.get('late_result') and not s.get('late_sent') and time.time() <= s.get('late_until', 0))

    def send_late_battle_data(self, f):
        """Battle Data opened after its run went without it (the run waits BATTLE_DATA_WAIT, then goes with the result
        screen alone; with Automation off nothing presses View Battle Data, and a player who stays on the result screen
        longer opens it late): send the run again, once, with its Battle Data and the result picture kept for it. The site
        updates the run it has (the same run id). Up to LATE_BATTLE_DATA_S after the first send."""
        path = self.run_path(f)
        try:
            with open(path, encoding='utf-8') as fh: s = json.load(fh)
        except (OSError, ValueError): return
        bd = s.get('battle_data') or {}
        if not self.late_open(s) or not bd.get('players'): return
        better = self.beats_best(s)                               # Battle Data's own bests (dps, break, taken, heal)
        s['new_bests'] = sorted(set(s.get('new_bests') or []) | set(better)); s['late_sent'] = True
        S.write_json(path, s, indent=1, ensure_ascii=False)
        log.info(f"leaderboard: Battle Data opened after the run was sent - sending it again with its Battle Data"
                 f"{' (a new best: ' + ', '.join(better) + ')' if better else ''}")
        self.send_result(S.run_payload(s, self.uid), s['late_result'], battle=bd.get('screenshot'), party=s.get('party_screenshot'))

    @staticmethod
    def sweep_late_results(now=None):
        """The result pictures kept for a late Battle Data (logs/screens/late_*): gone once their window is over."""
        d = os.path.join(HERE, 'logs', 'screens'); now = now or time.time()
        try: names = [n for n in os.listdir(d) if n.startswith('late_')]
        except OSError: return
        for n in names:
            try:
                if now - os.path.getmtime(os.path.join(d, n)) > LATE_BATTLE_DATA_S + 60: os.remove(os.path.join(d, n))
            except OSError: pass

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
        self.sweep_late_results()
        # going without Battle Data: a copy of the result picture is kept LATE_BATTLE_DATA_S, so a Battle Data opened
        # later can still follow the run (the site takes Battle Data only with a result picture; the sent one is deleted)
        shot = (s.get('result') or {}).get('screenshot')
        if shot and not (s.get('battle_data') or {}).get('screenshot') and self.submitter and (self.submitter.config or {}).get('screenshots'):
            late = os.path.join('logs', 'screens', 'late_' + os.path.basename(shot))
            try:
                shutil.copyfile(os.path.join(HERE, shot), os.path.join(HERE, late))
                s['late_result'] = late; s['late_until'] = time.time() + LATE_BATTLE_DATA_S
            except OSError: pass
        S.write_json(path, s, indent=1, ensure_ascii=False)
        self.send_result(S.run_payload(s, self.uid), (s.get('result') or {}).get('screenshot'),
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

    def send_waiting_run(self):
        """The meter closing (aLogs quit, restarted or updating) while a run still waits for its Battle Data: decide it
        now on what it has. It's then in the send queue on disk and goes at once or on the next start; before, the run
        was dropped (12 Nov: a new best record lost, aLogs quit 39 s after its result screen, inside the 45 s wait)."""
        if getattr(self, 'pending_run', None):
            log.info('leaderboard: the meter is closing - sending the run that was waiting for its Battle Data')
            self.decide_run(self.pending_run[0])

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

    def presence_tick(self, now):
        """Online / AFK on your profile while the game runs (presence.py): sent when it changes and every 2 minutes
        (the site forgets it after 5), and "offline" once the game closes."""
        # Dev switch queue-wait-fight: the sender is told, about once a second, whether a fight is on (Submitter.hold_retry)
        if getattr(self, 'submitter', None) and D.on('queue-wait-fight'):
            self.submitter.fight_seen = (time.time(), bool(getattr(self, 'fight', None)))
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

# The leaderboards' methods the core calls: an error in one is the plug-in's (logged as [leaderboards], counted; three
# in a minute stop the plug-in until its switch is turned off and on), not the reading loop's.
ENTRY = {'auto_active': False, 'check_pending_run': None, 'egg_fade': None, 'egg_hold_end': None, 'end_auto': None,
         'link_poll': None, 'link_read': None, 'open_battle_data': None, 'presence_tick': None, 'proof_crop': None,
         'prove_name': None, 'sent_notice': None, 'sim_check': None, 'sim_gate': None, 'sim_tour_step': False,
         'start_auto': None, 'proof_with_overlay': None, 'run_to_send': False, 'send_late_battle_data': None, 'sim_release': None, 'sim_send': None,
         'sim_tour_start': False}
for _name, _fallback in ENTRY.items():
    setattr(LeaderboardsMixin, _name, guarded('leaderboards', _fallback)(getattr(LeaderboardsMixin, _name)))

__all__ = ['EGG_BLACK', 'GIZMO_LIST_BOX', 'HOLD_MIN', 'PROOF_BYTES', 'RESULT_PAUSE', 'SIM_GATE_GONE', 'SIM_PRESS_EARLY', 'SIM_VICTORY_WAIT', 'gizmo_list_same', 'gizmo_list_sig', 'save_proof', 'sim_gate_pill', 'LeaderboardsMixin']
