"""Each plug-in has its own switch and its own health (plugins.py, from plugins.json): the meter reads the game while a
plug-in that needs it is on, each notice shows while its owner is on, and an error in a plug-in's code is that plug-in's
(three in a minute stop it, the others go on). Usage: python tests/plugin_switches.py"""
import os, sys, logging
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__))); sys.path.insert(0, HERE)
import plugins as P
logging.getLogger('meter').addHandler(logging.NullHandler()); logging.getLogger('meter').propagate = False

failed = False
def check(name, ok):
    global failed
    print(('PASS ' if ok else 'FAIL ') + name)
    failed |= not ok

on, off = {'leaderboard': {'submit': True}}, {'leaderboard': {'submit': False}}
check('the list: DPS meter, Sanctum Helper, leaderboards', list(P.PLUGINS) == ['dps', 'sanctum', 'leaderboards'])
check('meter on: on', P.meter_mode(False, off) == 'on' and P.meter_mode(False, on) == 'on')
check('meter off, leaderboards on: quiet (records still read and sent)', P.meter_mode(True, on) == 'quiet')
check('meter off, leaderboards off: rest', P.meter_mode(True, off) == 'rest' and P.meter_mode(True, {}) == 'rest')
check('Sanctum Helper on alone: rest (it reads the game itself, not through the meter)', P.meter_mode(True, {'sanctum': {'enabled': True}}) == 'rest')

owners = P.notice_owners({'leaderboard': {'submit': True, 'notify_sent': False}, 'sanctum': {'enabled': True}})
check("notices: each follows its owner's switch and its own setting", owners == {'button': True, 'sanctum': True, 'record': False})
check('notices: the button\'s follow notifications.button', P.notice_owners({'notifications': {'button': False}})['button'] is False)

calls = []
@P.guarded('leaderboards', fallback='nothing')
def flaky(x):
    calls.append(x)
    if x == 'boom': raise ValueError('a bad read')
    return x
check('a guarded call gives its result', flaky('ok') == 'ok')
check('an error is the plug-in\'s: the fallback comes back, nothing is raised', flaky('boom') == 'nothing')
check('one error doesn\'t stop it', not P.HEALTH.failed('leaderboards') and flaky('ok') == 'ok')
flaky('boom'); flaky('boom')
check('three in a minute stop it', P.HEALTH.failed('leaderboards'))
n = len(calls)
check('stopped: its code isn\'t run, the fallback comes back', flaky('ok') == 'nothing' and len(calls) == n)
check('stopped: the reading goes on without it (DPS meter off: rest)', P.meter_mode(True, on) == 'rest')
check('stopped: its notices are dropped', P.notice_owners(on)['record'] is False)
check('the other plug-ins go on', not P.HEALTH.failed('dps') and P.meter_mode(False, on) == 'on')
w = P.SwitchWatch(); w.tick(on, True); w.tick(off, True); w.tick(on, True)
check('its switch off and on again: another go', not P.HEALTH.failed('leaderboards') and flaky('ok') == 'ok')
check('its status says why', P.HEALTH.status('leaderboards')['last_error'].startswith('flaky: ValueError'))

# the overlay: none of the DPS meter's windows while it's off (or stopped), only the notices, after the tab-out check
src = open(os.path.join(HERE, 'dps_overlay.py'), encoding='utf-8').read()
tick = src[src.index('    def poll_tick():'):src.index('    th = threading.Thread(target=meter.run')]
quiet = tick[tick.index("if getattr(meter, 'quiet', False)"):tick.index("dps_at['quiet'] = False")]
check('quiet comes after the tab-out check', tick.index('front = game_in_front()') < tick.index("if getattr(meter, 'quiet', False)"))
check("quiet hides the meter's windows", all(h in quiet for h in ('hide_alert()', 'hide_hp()', 'hide_dps()')))
check('quiet keeps only the notices', "if k == 'toast': toast['queue'].append(d)" in quiet and 'toast_tick(); return' in quiet)
check('a stopped DPS meter hides its windows too', "HEALTH.failed('dps')" in quiet.split('\n')[0])
lead = open(os.path.join(HERE, 'leaderboards.py'), encoding='utf-8').read()
check("the leaderboards' entry points are guarded", "guarded('leaderboards', _fallback)" in lead)
import leaderboards
check('...every one the core calls', all(getattr(getattr(leaderboards.LeaderboardsMixin, n), 'guarded', None) == 'leaderboards' for n in leaderboards.ENTRY))
print('ALL PASS' if not failed else 'SOME FAILED'); sys.exit(1 if failed else 0)
