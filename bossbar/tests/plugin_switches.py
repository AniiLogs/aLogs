"""Each plug-in has its own switch (live.meter_mode): the DPS meter off with the leaderboards on still reads the game
(quiet), both off rest, the meter on reads and shows. Usage: python tests/plugin_switches.py"""
import ast, os, sys
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
src = open(os.path.join(HERE, 'live.py'), encoding='utf-8').read()
fn = next(n for n in ast.parse(src).body if isinstance(n, ast.FunctionDef) and n.name == 'meter_mode')
ns = {}; exec(compile(ast.Module([fn], []), 'live.py', 'exec'), ns); meter_mode = ns['meter_mode']

failed = False
def check(name, ok):
    global failed
    print(('PASS ' if ok else 'FAIL ') + name)
    failed |= not ok

check('meter on, leaderboards off: on', meter_mode(False, {'leaderboard': {'submit': False}}) == 'on')
check('meter on, leaderboards on: on', meter_mode(False, {'leaderboard': {'submit': True}}) == 'on')
check('meter off, leaderboards on: quiet (records still read and sent)', meter_mode(True, {'leaderboard': {'submit': True}}) == 'quiet')
check('meter off, leaderboards off: rest', meter_mode(True, {'leaderboard': {'submit': False}}) == 'rest')
check('meter off, no leaderboard settings: rest', meter_mode(True, {}) == 'rest')
# the overlay's quiet branch: none of the meter's windows, only the notices, after the tab-out check
tick = src[src.index('    def poll_tick():'):src.index('    th = threading.Thread(target=meter.run')]
quiet = tick[tick.index("if getattr(meter, 'quiet', False):"):tick.index("dps_at['quiet'] = False")]
check('quiet comes after the tab-out check', tick.index('front = game_in_front()') < tick.index("if getattr(meter, 'quiet', False):"))
check("quiet hides the meter's windows", all(h in quiet for h in ('hide_alert()', 'hide_hp()', 'hide_dps()')))
check('quiet keeps only the notices', "if k == 'toast': toast['queue'].append(d)" in quiet and 'toast_tick(); return' in quiet)
print('ALL PASS' if not failed else 'SOME FAILED'); sys.exit(1 if failed else 0)
