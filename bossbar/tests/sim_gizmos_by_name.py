"""Holo-Battle Sim: the Gizmo screen is read even when the Victory screen's gizmo icons weren't recognised.

    python tests/sim_gizmos_by_name.py

13 Nov: the gizmo row was read while it faded in, matched no icon, and the Gizmo screen never opened (the tour wanted
every icon's series first). The Gizmo screen lists each pick by name, and the game data says each name's series, so
the tour opens anyway and the run's series come from the names read. Made-up names. Exit code 1 on any failure.
"""
import os, sys, tempfile, time, types
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_gz_')
import live, leaderboards as LB

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

def meter(gizmos):
    m = live.Meter.__new__(live.Meter)
    m.args = types.SimpleNamespace(replay=None, cfg={'leaderboard': {'submit': True, 'automation': True},
                                                     'results': {'sim_gizmos': True, 'hold_inputs': False, 'hold_seconds': 60}})
    m.hwnd = 1; m.sim_tour = None; m.sim_ctl = None; m.space = 's'
    m.icons = types.SimpleNamespace(sim={'0': {'name': 'Energy'}, '2': {'name': 'Wealth'}},
                                    sim_gizmos={'Spark Coil': ('0', 'Rare'), 'Gold Bell': ('2', 'Epic'), 'Coin Rain': ('2', 'Rare')})
    m.sim_pending = ({'setup': {'gizmos': gizmos}}, 'shot.jpg')
    m.sim_control = lambda now, why: setattr(m, 'sim_ctl', {'why': why}) or m.sim_ctl
    m.sim_rarity = lambda n: 'Rare'
    m.sim_count = lambda rows: len(rows)
    m.sent = []; m.sim_send = lambda note=None: m.sent.append(note)
    m.sim_release = lambda why: None
    return m
LB.wc.is_foreground = lambda hwnd: True

# no icon recognised, no counts read: the tour still starts
m = meter(None)
check('tour starts without recognised icons', m.sim_tour_start(time.time()), True)
m.sim_tour['read'] = {'2': ['Gold Bell', 'Coin Rain'], '0': ['Spark Coil']}
m.sim_tour_end('done')
g = m.sim_pending[0]['setup']['gizmos']
check('series from the names, in series order', [(x['series'], x['sid']) for x in g], [('Energy', '0'), ('Wealth', '2')])
check('picks under each series', [[p['name'] for p in x['picked']] for x in g], [['Spark Coil'], ['Gold Bell', 'Coin Rain']])
check('sent once', m.sent, ['3 gizmos'])

# counts read under the icons, icons not recognised: the counts go with the series when there are as many
m = meter([{'count': 1}, {'count': 2}])
m.sim_tour_start(time.time())
check('wants the counted picks', m.sim_tour['want'], 3)
m.sim_tour['read'] = {'0': ['Spark Coil'], '2': ['Gold Bell', 'Coin Rain']}
m.sim_tour_end('done')
check('counts kept, complete', [(x['count'], x['complete']) for x in m.sim_pending[0]['setup']['gizmos']], [(1, True), (2, True)])

# icons recognised: as before
m = meter([{'series': 'Wealth', 'sid': '2', 'count': 2}])
m.sim_tour_start(time.time())
m.sim_tour['read'] = {'2': ['Gold Bell', 'Coin Rain']}
m.sim_tour_end('done')
check('recognised series kept', [(x['sid'], len(x['picked'])) for x in m.sim_pending[0]['setup']['gizmos']], [('2', 2)])

if fails:
    print('FAIL\n  ' + '\n  '.join(fails)); sys.exit(1)
print('sim_gizmos_by_name: all checks pass')
