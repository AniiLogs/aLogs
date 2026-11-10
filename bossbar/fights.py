"""Following a boss fight on its HP bar (core, shared by the plug-ins): the fight, the boss, its damage and DPS, the
kill. The DPS meter shows it (dps_overlay.py); the leaderboards send the run it becomes (leaderboards.py)."""
from core import *
import devflags as D

KILL_SOUND_WINDOW = (2.5, 6.0)  # the boss kill sound counts when it started this many seconds before the countdown
SLIDE_IN_RISE = 0.15    # Dev switch fight-bar-slide-in: how much fuller than the fight's lowest a reading must be to throw that lowest out
KILL_TO_COUNTDOWN = 3.0 # seconds from the killing blow to the game's Catch Countdown (the kill-cam: no HUD, slow
FIGHT_INTERVAL = 1 / 30 # seconds between frames during a fight
DAMAGE_NUMBERS = False  # the damage-number recorder (dmgnumbers.py, capture.damage_numbers): paused, off whatever the setting
                        # least this long after it, so a one-off effect lasting a few fast frames can't confirm itself
LEVEL_CONFIRM = 3       # s an Interlink level other than the season's must be read alone before a fight starts on it
XN_STALE = 40           # s a boss rush counter read keeps bounding refills (the late bars take 15-25 s each)

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

HP_BAR = (0.400, 0.915, 0.604, 0.9519)    # your HP bar at the bottom middle with its number on it, in the 16:9 UI area
HP_TEXT = re.compile(r'(\d[\d ]*)\s*/\s*(\d[\d ]*)')

# How far down the screen an Interlink boss bar's centre goes (share of its height): 186-197 px of 2160 in 14,000
# readings (5-6 Nov). The Party screen's title banner that passed as a bar sat at 454-505.
TOP_BAR = 0.15


def hp_number(ocr, strip, u):
    """Your HP as the game writes it on the bar ("12643/12694") in a picture of HP_BAR: (current, max), or None."""
    w, h = strip.size
    text = ocr.read({'hp': R.prep_white(strip, (w * 0.30, h * 0.36, w * 0.70, h * 0.80), u / 2000)}).get('hp', '')
    m = HP_TEXT.search(text.replace('O', '0').replace('o', '0'))
    return (int(m.group(1).replace(' ', '')), int(m.group(2).replace(' ', ''))) if m else None

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
        self.rise_at = None          # Dev switch fight-bar-slide-in: when a reading far above seg_fill was first seen
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
        # Dev switch fight-bar-slide-in. A world boss's bar can read nearly empty, in its own red, as it slides in, for longer
        # than the check above covers. 'HP only drops' then keeps that low reading, no drop counts as a hit until the bar falls
        # below it, and the clock starts far too late (8 Nov: at 5.4% after 15.5 s, a 2 s fight at 178,203 DPS). Nobody has hit
        # it yet and two readings say it's far fuller: the low one was the bar arriving.
        if not self.hit and not self.bossrush and fill is not None and fill - self.seg_fill > SLIDE_IN_RISE and D.on('fight-bar-slide-in'):
            if self.rise_at is None: self.rise_at = now
            elif now - self.rise_at >= CONFIRM_GAP:
                note = (note + f' low reading {self.seg_fill * 100:.1f}% dropped: the bar was sliding in').strip()
                self.seg_fill = fill; self.low_pending = None; self.first_damage = None; self.rise_at = None; self.samples.clear()
        elif self.rise_at is not None: self.rise_at = None
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


class FightsMixin:
    """Following fights: the Meter's (live.py) methods for it, sharing its state."""

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
                if D.on('kill-sound-log'): self.kill_sound_note(heard)
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
            path = os.path.join(d, os.path.splitext(os.path.basename(f.log.name))[0] + '.png')
            if im is getattr(self, 'canvas', None): im = im.copy()    # (never for a whole frame: the next look draws on that one)
            def save():                                 # a whole PNG: the slow part, on the writer's thread with proof-off-thread
                try: im.save(path)
                except Exception as e: log.info(f'bar sample not saved: {e!r}')
            picture_job(save)
        except Exception as e:
            log.info(f'bar sample not saved: {e!r}')

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

    def overtime_limit(self, now):
        """The run's clock when the leader's Special Offering on the Party screen read as Overtime (its own icon): the
        default plus Overtime's extra minute. For when the clock itself wasn't read as the fight started; else None."""
        recent = self.party and now - self.party['at'] < 900
        picked = recent and any(b.get('name') == 'Overtime' for b in self.party.get('buffs') or [])
        return self.args.duration + OVERTIME_S if picked else None

    def save_nobar(self, im, now):
        """Diagnostics: in an Interlink room (game log) but no boss bar found - keep a few full frames to see why."""
        if not self.args.cfg['capture']['nobar_frames'] or not self.bossrush_space: return   # Interlink only, not the lobby
        if now - self.last_nobar < 5 or self.nobar_count >= 30: return
        self.last_nobar = now; self.nobar_count += 1
        d = os.path.join(HERE, 'logs', 'nobar'); name = time.strftime('%Y%m%d_%H%M%S')
        save_picture(im, 'nobar', name)
        with open(os.path.join(d, name + '.txt'), 'w', encoding='utf-8') as fh: fh.write(R.LAST_FAIL[0] + '\n')

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
        path = os.path.join(f.frames, f'{now - f.created:07.2f}.png')
        if not D.on('proof-off-thread'): crop.save(path); return
        crop.load()                                     # its own pixels now: the frame it's cut from may be drawn over
        picture_job(lambda: crop.save(path))            # (an error there is the writer's log line, not this look's)

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

    def kill_sound_note(self, heard):
        """Dev switch kill-sound-log: why this kill's sound was or wasn't used. In 24 log files it never was, and nothing said why.
        The game plays it with every kill-cam (ClientCombatEntityComponent's kill performance has no condition on it), so
        it's the listening: no stream, or a match too low in the fight's din."""
        try:
            at = self.catch_seen
            on = bool(self.args.cfg.get('audio_cues', {}).get('boss_kill'))
            used = 'used' if heard is not None else 'not used'
            c = wc.audio.cue(); now = wc.audio.listening()
            if now is None:
                log.info(f"boss kill sound {used}: setting {'on' if on else 'off'}, the app's listening can't be read"); return
            state, audio_at, best, best_at = now
            parts = [f"setting {'on' if on else 'off'}", {0: 'not listening', 1: 'listening', 2: "can't listen"}.get(state, f'state {state}'),
                     f'game audio last heard {at - audio_at:.1f}s before the Catch Countdown' if audio_at else 'no game audio heard yet',
                     f'best match {best:.2f} (a kill needs 0.55) {at - best_at:.1f}s before it' if best_at else 'no match measured',
                     f'newest cue {at - c[1]:.1f}s before it (match {c[2]:.2f}; counts {KILL_SOUND_WINDOW[0]}-{KILL_SOUND_WINDOW[1]} s)' if c else 'no cue yet']
            log.info(f'boss kill sound {used}: ' + ', '.join(parts))
        except Exception as e: log.info(f'boss kill sound: no note: {e!r}')

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

    def fight_interval(self):
        """Seconds between frames in a fight: DPS Meter › Meter › frames a second in a fight: 30 (the default since B6)
        or 60 for the finest timing. A player's game stuttered at 4K with the meter at 60 (B5); the bar's HP and the
        damage read the same at 30."""
        cfg = self.args.cfg
        rate = cfg.get('capture', {}).get('fight_rate', 30)
        chosen = 'capture.fight_rate' in (cfg.get('user_set') or [])
        if not (rate == 60 and chosen): rate = 30              # 60 only when picked (it was the default before B6)
        return FIGHT_INTERVAL if rate == 30 else 1 / 60

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
            W, H = im.size; u = min(W, H * 16 / 9); self.fight_team(im, time.time(), u, (W - u) / 2, H)
        self.last_screen = (self.origin[0], self.origin[1], self.frame_size[0], self.frame_size[1])
        # max_hp and damage are the run's (a two-boss fight's both bosses); boss_hp is the max HP of the boss on the bar
        self.out('fight', {'title': f.title, 'level': f.level, 'max_hp': f.max_hp, 'boss_hp': f.boss_hp, 'xn': f.seg, 'fill': f.seg_fill,
                           'damage': f.damage, 'elapsed': f.elapsed, 'avg': f.avg, 'recent': f.recent,
                           'boss': f.boss_of(),
                           'bossrush': f.bossrush, 'holosim': self.holosim_space, 'seg_size': si.get('size_hp'), 'seg_left': si.get('left_hp'),
                           'bar': (self.origin[0] + bar['left_bracket'], self.origin[1] + bar['yc'], R.label_span(bar['left_bracket'], bar['span'], self.frame_size[0])),
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

    def instant_start(self, xn, now, low=False):
        """Interlink: the prep screen just named the boss and every Interlink boss has the same level as the last one,
        so its fight starts on the very first frame of its bar - no waiting for the name above the bar to be read.
        That reading still runs and corrects the boss if it disagrees (identify's three-reads rule).
        low: the bar sits lower than an Interlink boss bar does (TOP_BAR). The Party screen's orange title banner over
        a player passes as a bar there, and its counter spot can read a digit ('1'), which opened a fight before the
        run (6 Nov, 3 times). So a low bar starts a fight only on a full counter (x21, or one down if the first
        frames were missed: a fight starts at full HP); a bar up top starts it on any counter read (x21 can read '2'
        with its '1' cut off, 1 Nov). Anything else waits for the name over the bar."""
        if self.fight or xn is None or not self.rush_level or not self.prep_boss or now - self.prep_boss[0] > 180: return
        if low and xn < R.xn_top(self.grades) - 1: return
        row = self.prep_boss[1]
        self.start_fight(row, self.rush_level, row['bossTitle_en'], True, f'from the prep screen, xN {xn}')
        self.prep_boss = None

__all__ = ['TOP_BAR', 'BreakTracker', 'DAMAGE_NUMBERS', 'FIGHT_CSV', 'FIGHT_INTERVAL', 'Fight', 'HANDOFF_XN', 'HP_BAR', 'HP_TEXT', 'KILL_SOUND_WINDOW', 'KILL_TO_COUNTDOWN', 'LEVEL_CONFIRM', 'MAX_CLOCK', 'OVERTIME_S', 'Timer', 'XN_STALE', 'hp_number', 'FightsMixin']
