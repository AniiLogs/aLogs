"""Interlink's HP bars against the game files: the grade table (bossrush_grade_data, 22 rows x 3 columns), the column
each boss's bars follow, the xN counter (x21 .. x1) and the damage the meter reads off a bar.

    python tests/interlink_grades.py

- The table is loaded from the installed game's data (gamedata.read_grades on pmdata.bin) and must equal the shipped
  boss_rush_grades.csv and what reader.load_grades gives, number for number. Without a game install only the shipped
  table is checked.
- Column choice, as the game makes it (BossRushUtils.lua local getBossHpLossPercentField, line 306, with
  ClientBossRushDungeon.getBossSpawnIndex, line 232): a one-boss stage's boss -> bossHpLossPercent, a two-boss
  stage's first boss (bossId) -> bossHpLossPercent_2, its second boss (bossId_2) -> bossHpLossPercent_3.
- Where each bar starts, against the game's own Lua run in its xlua.dll (two-boss audit, lua-dis/run_hpfield.out.txt:
  "bar N starts at X% lost", and the bar a boss is in at 50%, 95% and 39.76% of its HP lost).
- The meter's damage read (reader.damage_dealt and Fight.update) turns each bar the game shows back into the HP lost.
Nothing is sent and nothing lands in your logs: the meter's data folder is a temporary one. Exit code 1 on any failure.
"""
import os, shutil, sys, tempfile, time
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
TMP = tempfile.mkdtemp(prefix='alogs_grades_')
os.environ['ALOGS_DATA'] = TMP                  # before settings is imported
import live, reader as R, gamedata

BASE, FIRST, SECOND = 'bossHpLossPercent', 'bossHpLossPercent_2', 'bossHpLossPercent_3'
# week -> (a one-boss stage's boss, the two-boss middle's first boss, its second boss); 4000 has no two-boss fight
STAGES = {'4003': ('80000109', '80000120', '80000119'), '4004': ('80000097', '80000122', '80000121'),
          '4005': ('80000113', '80000124', '80000123'), '4000': ('80000112', None, None)}
# The game's Lua run (lua-dis/run_hpfield.out.txt), spawn index nil / 1 / 2: getBossCurHpState walked from full HP
# to 0 in steps of 50 of 1,000,000, printing 'bar@share lost' wherever the bar changes.
LUA_WALK = {
    BASE: '1@0.0000 2@0.0044 3@0.0089 4@0.0144 5@0.0199 6@0.0266 7@0.0332 8@0.0410 9@0.0476 10@0.0619 11@0.0786 '
          '12@0.0953 13@0.1191 14@0.1502 15@0.1920 16@0.2240 17@0.2540 18@0.2840 19@0.3416 20@0.4136 21@0.5000',
    FIRST: '1@0.0000 2@0.0117 3@0.0234 4@0.0380 5@0.0526 6@0.0701 7@0.0876 8@0.1080 9@0.1257 10@0.1634 11@0.2074 '
           '12@0.2514 13@0.3142 14@0.3906 15@0.4644 16@0.5595 17@0.6228 18@0.6862 19@0.7494 20@0.8445 21@0.9500',
    SECOND: '1@0.0000 2@0.0024 3@0.0047 4@0.0070 5@0.0094 6@0.0118 7@0.0141 8@0.0164 9@0.0407 10@0.0587 11@0.0767 '
            '12@0.0944 13@0.1120 14@0.1296 15@0.1473 16@0.1807 17@0.2140 18@0.2557 19@0.2975 20@0.3476 21@0.3976'}
# the same run's whole chain: the bar a boss is in at 50% / 95% / 39.76% of its HP lost
LUA_BAR_AT = {BASE: (21, 21, 19), FIRST: (15, 21, 14), SECOND: (21, 21, 21)}

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

class At:
    def __init__(self, t): self.t = t
    def __enter__(self): self.real = time.time; time.time = lambda: self.t
    def __exit__(self, *a): time.time = self.real

def inside(week):
    import json
    with open(R.CYCLES_FILE[0], encoding='utf-8') as f: cycles = json.load(f)
    return next(c for c in cycles if str(c['cycle']) == week)['start'] + 2 * 86400

def bar_at(loss, cum):
    """The bar (grade being worked toward) at this share of HP lost, as BossRushUtils.getBossCurHpState (line 317)
    finds it: the first grade whose threshold maxHp * (1 - rate) is still below the HP left."""
    return next((g for g in range(1, len(cum)) if 1 - cum[g] < 1 - loss), len(cum) - 1)

def lua_walk(cum, max_hp=1000000, step=50):
    """The Lua run's walk with the meter's table: 'bar@share lost' where the bar changes (string.format %.4f). The bar
    is found as getBossCurHpState finds it (bar_at, in whole HP as the Lua run had it)."""
    out, last = [], None
    for lost in range(0, max_hp + 1, step):
        g = next((i for i in range(1, len(cum)) if max_hp * (1 - cum[i]) < max_hp - lost), len(cum) - 1)
        if g != last: out.append('%d@%.4f' % (g, lost / max_hp)); last = g
    return ' '.join(out)

def table_checks():
    game_path = gamedata.find_game_data()
    shipped = gamedata.shipped_grades()
    check('shipped table: rows', len(shipped), 22)
    if game_path:
        game = gamedata.read_grades(game_path)
        check('game table: rows', len(game), 22)
        for c in (BASE, FIRST, SECOND):
            check(f'shipped {c} = game', [r[c] for r in shipped], [r[c] for r in game])
        check('gamedata.grades_drift', gamedata.grades_drift(game_path), [])
        R.GAME_GRADES[0] = game
        check('load_grades from the game rows = shipped', R.load_grades(), {c: [float(r[c]) for r in shipped] for c in (BASE, FIRST, SECOND)})
        R.GAME_GRADES[0] = None
    else: print('no game install found: the table is checked against its own shape only')
    grades = R.load_grades()
    check('columns', sorted(grades), sorted((BASE, FIRST, SECOND)))
    check('xn_top (the counter at full HP)', R.xn_top(grades), 21)
    for c in (BASE, FIRST, SECOND):
        cum = grades[c]
        check(f'{c}: grade 0 and Eliminate', (cum[0], cum[-1]), (0.0, 1.0))
        check(f'{c}: never goes down', all(b >= a for a, b in zip(cum, cum[1:])), True)
        # every bar's start, as the game's own code walked it; bar 21 (Eliminate, x1) starts at grade 20's threshold:
        # 50% on a one-boss stage, 95% for a two-boss stage's first boss, 39.76% for its second
        check(f'{c}: where each bar starts (Lua run)', lua_walk(cum), LUA_WALK[c])
        check(f'{c}: bar at 50% / 95% / 39.76% lost (Lua run)', tuple(bar_at(x, cum) for x in (0.5, 0.95, 0.3976)), LUA_BAR_AT[c])
    # the game's table is rounded to 15 digits, so compare to 12
    check('second boss: bar 20 ends at 33/83', round(grades[SECOND][20], 12), round(33 / 83, 12))
    return grades

def column_checks():
    for week, (solo, first, second) in STAGES.items():
        with At(inside(week)):
            check(f'{week} one-boss stage {solo}', R.grade_column(solo), BASE)
            if first:
                check(f'{week} two-boss first {first}', R.grade_column(first), FIRST)
                check(f'{week} two-boss second {second}', R.grade_column(second), SECOND)
    # outside their weeks (the schedule's latest week with them decides): still their own columns
    with At(inside('4000')):
        check('80000120 during 4000', R.grade_column('80000120'), FIRST)
        check('80000119 during 4000', R.grade_column('80000119'), SECOND)
        check('80000115 (one-boss) during 4000', R.grade_column('80000115'), BASE)
    check('no boss', R.grade_column(None), BASE)

def damage_checks(grades):
    # every bar of every column: what the game shows (xN, fill of that bar) read back as the HP lost
    worst = 0.0
    for c in (BASE, FIRST, SECOND):
        cum = grades[c]
        for k in range(1, 1000):
            loss = k / 1000; g = bar_at(loss, cum); xn = len(cum) - g
            fill = (cum[g] - loss) / (cum[g] - cum[g - 1])
            dmg, seg = R.damage_dealt(1_000_000_000, fill, xn, cum)
            worst = max(worst, abs(dmg / 1e9 - loss))
            if seg['grade'] != g: fails.append(f'{c}: {loss:.3f} lost read as grade {seg["grade"]}, the game is on {g}'); break
    check('damage read back, worst error in share of max HP (< 1e-9)', worst < 1e-9, True)
    check('x21 is grade 1', R.damage_dealt(100, 1.0, 21, grades[BASE])[1]['grade'], 1)
    check('x22 is no bar', R.damage_dealt(100, 1.0, 22, grades[BASE]), (None, None))
    for t, want in (('x21 x21 x21', 21), ('x17', 17), ('x20 / x2O', 20), ('x77', 11), ('x27', 21), ('x16', 16), ('x1', 1), ('x99', None)):
        check(f'parse_xn {t!r}', R.parse_xn(t), want)

def fight_checks(grades):
    """A Fight for each kind of boss, from its full bar (x21) to half of its next bar (x20 at 50%)."""
    catalog = R.load_catalog()
    row = lambda pid: next(r for r in catalog if r['table'] == 'puppet_data' and r['puppetId'] == pid)
    for pid, col in (('80000109', BASE), ('80000120', FIRST), ('80000119', SECOND)):
        with At(inside('4003')):
            f = live.Fight('test', 60, row(pid), 1_000_000, True)
        try:
            check(f'Fight {pid} column', f.column, col)
            red = (230, 100, 100); t = 1000.0
            f.update(1.0, red, red, 21, None, grades, 180, at=t)
            check(f'Fight {pid} starts on x21', (f.seg, f.started_at_top, f.damage), (21, True, 0))
            for i in range(1, 4): f.update(0.5, red, red, 20, None, grades, 180, at=t + i * 0.2)
            want = round((grades[col][1] + 0.5 * (grades[col][2] - grades[col][1])) * 1_000_000)
            check(f'Fight {pid} at x20, half the bar', (f.seg, f.damage), (20, want))
        finally: f.close()

def main():
    grades = table_checks()
    column_checks()
    damage_checks(grades)
    fight_checks(grades)
    print('grade table, columns and damage: ' + ('all right' if not fails else f'{len(fails)} failures'))
    for f in fails: print('  FAIL', f)
    return 1 if fails else 0

if __name__ == '__main__':
    try: code = main()
    finally: shutil.rmtree(TMP, ignore_errors=True)
    sys.exit(code)
