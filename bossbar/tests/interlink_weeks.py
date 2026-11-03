"""Interlink weeks 4000-4005 through the meter's own helpers: every boss found from its bar title, in its slot, a
two-boss middle fight's order and partner, and the full rating (star cap) each boss gets; and the shipped boss list
against the game data (the build's check).

    python tests/interlink_weeks.py

The expected values are written out below from the game files (build 3857679): bossrush_cycle_data[4000..4005]
bossLeft / bossMid / bossRight -> bossrush_guanka_data bossId (+ bossId_2 for weeks 4003-4005's middle) ->
puppet_data BossShowName. The meter reads the shipped boss_rush_cycles.json, so this also checks that file. Nothing is
sent and nothing lands in your logs: the meter's data folder is a temporary one. Exit code 1 on any failure.
"""
import os, shutil, sys, tempfile, time
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
TMP = tempfile.mkdtemp(prefix='alogs_weeks_')
os.environ['ALOGS_DATA'] = TMP                  # before settings is imported: no star_caps.json, no game_cycles.json
import live, reader as R

# week: (left, middle, right); a two-boss middle is (first boss, second boss). Each boss is (puppetId, its bar title).
WEEKS = {
    '4000': (('80000098', 'Alpha Scorchhowl'), ('80000112', 'Alpha Glameep'), ('80000096', 'Alpha Minespine')),
    '4001': (('80000115', 'Alpha Jabster'), ('80000114', 'Omega Jeburst'), ('80000103', 'Alpha Luminelle')),
    '4002': (('80000116', 'Alpha Sugartini'), ('80000111', 'Alpha Magmarex'), ('80000102', 'Alpha Panpanta')),
    '4003': (('80000109', 'Omega Sherro'), (('80000120', 'Alpha Blazen'), ('80000119', 'Omega Tucking')), ('80000101', 'Alpha Bouldus')),
    '4004': (('80000097', 'Alpha Tubster'), (('80000122', 'Alpha Glameep'), ('80000121', 'Alpha Jabster')), ('80000117', 'Alpha Sugartini')),
    '4005': (('80000113', 'Alpha Glameep'), (('80000124', 'Alpha Grizbo'), ('80000123', 'Omega Jeburst')), ('80000110', 'Omega Infergon')),
}
SIDE, CENTER = 20, 40          # sys_config_data BossRushMaxStarSideBoss 20 x BossRushStarMultiplierMidBoss 2 (from week 4000)

def bosses(week):
    """(slot, puppetId, title, order, partner) for each boss of the week."""
    for slot, entry in zip(('left', 'middle', 'right'), WEEKS[week]):
        if isinstance(entry[0], tuple):
            (a, ta), (b, tb) = entry
            yield slot, a, ta, 1, b
            yield slot, b, tb, 2, a
        else: yield slot, entry[0], entry[1], None, None

def inside(week):
    """A time in the middle of the week (its region-1 start + 2 days), from the meter's own schedule."""
    import json
    with open(R.CYCLES_FILE[0], encoding='utf-8') as f: cycles = json.load(f)
    c = next(c for c in cycles if str(c['cycle']) == week)
    return c['start'] + 2 * 86400

class At:
    """time.time() is `t` inside the block (the helpers ask for the week running now)."""
    def __init__(self, t): self.t = t
    def __enter__(self): self.real = time.time; time.time = lambda: self.t
    def __exit__(self, *a): time.time = self.real

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

def catalogs():
    """The shipped boss list, and the one read from the installed game when there is one (the meter uses that)."""
    import gamedata
    R.GAME_BOSSES[0] = None
    yield 'shipped list', R.load_catalog()
    game = gamedata.find_game_data()
    if game:
        keep = [(r['table'], r['puppetId']) for r in R.load_catalog() if r.get('monsterType') != '2']
        R.GAME_BOSSES[0] = gamedata.read_bosses(game, keep)
        yield 'game list', R.load_catalog()
        R.GAME_BOSSES[0] = None
    else: print('no game install found: the boss list read from the game is not checked')

def refresh_check():
    """live.refresh_cycles, one pass, on a data folder holding a week schedule written by an older aLogs (no bossIds2):
    it must be rewritten from the game data, and a file that already matches must be left alone. Needs the game
    installed; nothing is sent (upload_cycles is switched off here)."""
    import json, types, gamedata
    game = gamedata.find_game_data()
    if not game:
        print('no game install found: the week schedule rewrite (refresh_cycles) is not checked'); return
    path = os.path.join(live.HERE, 'boss_rush_cycles.json')
    with open(R.CYCLES_FILE[0], encoding='utf-8') as f: shipped = json.load(f)
    with open(path, 'w', encoding='utf-8') as f: json.dump([{k: v for k, v in c.items() if k != 'bossIds2'} for c in shipped], f)
    class Once:                                     # meter.stop: False for the pass, then True (no 6-hour wait)
        reads = 0
        @property
        def stop(self): Once.reads += 1; return Once.reads > 1
    meter = Once(); meter.args = types.SimpleNamespace(player_log=None); meter.catalog = R.load_catalog()
    real = (R.CYCLES_FILE[0], R.GAME_BOSSES[0], live.upload_cycles)
    live.upload_cycles = lambda *a, **k: None
    try:
        with At(inside('4003')): live.refresh_cycles(meter)
        with open(path, encoding='utf-8') as f: written = json.load(f)
        want = json.loads(json.dumps(gamedata.read_cycles(game)))
        check('refresh_cycles: an old data-folder schedule is rewritten from the game data (weeks that differ)',
              [c.get('cycle') for c, w in zip(written, want) if c != w] + ['count'] * (len(written) != len(want)), [])
        check('refresh_cycles: the meter reads it', R.CYCLES_FILE[0], path)
        check('refresh_cycles: 4003 Omega Tucking after the rewrite', R.stage_of('80000119', '4003'), ('middle', 2, '80000120'))
        before = os.stat(path).st_mtime_ns
        Once.reads = 0
        with At(inside('4003')): live.refresh_cycles(meter)
        check('refresh_cycles: a schedule that matches is left alone', os.stat(path).st_mtime_ns, before)
    finally: R.CYCLES_FILE[0], R.GAME_BOSSES[0], live.upload_cycles = real

def main():
    import gamedata
    n = 0
    # the bar's title -> the boss, as the meter picks it: a title shared by several entries (Alpha Glameep: 80000112
    # middle in 4000, 80000122 two-boss middle in 4004, 80000113 left in 4005; Alpha Jabster: 80000115 left in 4001,
    # 80000121 two-boss middle in 4004 while Alpha Tubster 80000097 is 4004's left) must give that week's
    for name, catalog in catalogs():
        for week in WEEKS:
            for slot, pid, title, order, partner in bosses(week):
                with At(inside(week)):
                    row, _ = R.pick_boss(catalog, title, True)
                check(f'{name}: {week} {title} pick_boss', row and row['puppetId'], pid)
    # the shipped list against the game data (what the build checks), else against the shipped week schedule
    game = gamedata.find_game_data()
    check(f"boss_catalog.csv against {'the game data' if game else 'boss_rush_cycles.json'}", gamedata.catalog_drift(game), [])
    for week in WEEKS:
        for slot, pid, title, order, partner in bosses(week):
            n += 1
            check(f'{week} {title} cycle_slot', R.cycle_slot(week, pid)[0], slot)
            check(f'{week} {title} stage_of({week})', R.stage_of(pid, week), (slot, order, partner))
            with At(inside(week)):
                check(f'{week} {title} active week', R.active_cycle_id(), week)
                check(f'{week} {title} stage_of (week now)', R.stage_of(pid), (slot, order, partner))
                check(f'{week} {title} clear_stars', live.clear_stars(pid), CENTER if slot == 'middle' else SIDE)
                check(f'{week} {title} clear_stars(int)', live.clear_stars(int(pid)), CENTER if slot == 'middle' else SIDE)
    # a boss outside its week: no slot that week; with no week given, the latest week that has it
    check('4003 Alpha Glameep 80000112 not in 4003', R.stage_of('80000112', '4003'), (None, None, None))
    with At(inside('4004')):
        check('Omega Tucking 80000119 during 4004 (not that week)', live.clear_stars('80000119'), SIDE)
        check('stage_of 80000119 during 4004 (latest week with it)', R.stage_of('80000119'), ('middle', 2, '80000120'))
    check('stage_of None', R.stage_of(None), (None, None, None))
    check('clear_stars None', live.clear_stars(None), SIDE)
    refresh_check()
    print(f'{n} bosses in weeks {min(WEEKS)}-{max(WEEKS)}: ' + ('all right' if not fails else f'{len(fails)} failures'))
    for f in fails: print('  FAIL', f)
    return 1 if fails else 0

if __name__ == '__main__':
    try: code = main()
    finally: shutil.rmtree(TMP, ignore_errors=True)
    sys.exit(code)
