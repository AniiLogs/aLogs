"""An Interlink fight starts at once from the prep screen's boss only where a boss bar can be: a low "bar" needs x21.

    python tests/prep_start.py

FightsMixin.instant_start opens the fight on the first frame of the bar when the prep screen named the boss. The Party
screen's orange title banner over a player passes as a bar and its counter spot can read a digit: on 6 Nov it read
'1' and opened three fights before the run. A fight starts at full HP, so only x21 (or x20, the first frames missed)
starts it from a bar lower than Interlink bars sit; one up top starts on any counter read (x21 read as '2', 1 Nov).
No digit, or a prep screen read over 3 minutes ago, starts nothing.
Stand-ins only (no screen, no game files). Exit code 1 on any failure.
"""
import os, sys, tempfile
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_prep_')
import live, reader as R

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

def started(xn, age=5, low=True):
    m = live.Meter.__new__(live.Meter)
    m.fight = None; m.rush_level = 60; m.grades = R.load_grades()
    m.prep_boss = (1000 - age, {'bossTitle_en': 'Alpha Jabster', 'puppetId': '80000115'})
    calls = []
    m.start_fight = lambda row, level, name, bossrush, how: calls.append(how)
    m.instant_start(xn, 1000, low=low)
    return bool(calls)

top = R.xn_top(R.load_grades())
check('full counter x21 starts', started(top), True)
check('x20 (first frames missed) starts', started(top - 1), True)
check("the banner's stray '1' doesn't", started(1), False)
check('x10 (mid-fight) doesn\'t', started(10), False)
check('no digit doesn\'t', started(None), False)
check('a prep screen read 4 minutes ago doesn\'t', started(top, age=240), False)

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('prep start: ok')
