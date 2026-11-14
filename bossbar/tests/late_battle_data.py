"""A Battle Data opened after its run went without it is sent with the run, once, inside the late window.

    python tests/late_battle_data.py

A run waits BATTLE_DATA_WAIT (45 s) for its Battle Data, then goes with the result screen alone. With Automation off
nothing presses View Battle Data, so a player who stays on the result screen opens it late: send_run keeps a copy of the
result picture (the sent one is deleted, and the site takes Battle Data only with a result picture), and
send_late_battle_data sends the run again with both, once, up to LATE_BATTLE_DATA_S later. Made-up numbers only.
Exit code 1 on any failure.
"""
import json, os, sys, tempfile, time, types
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_late_')
import live, leaderboards as LB

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

data = live.HERE
for d in ('fights', 'screens', 'battle_data'): os.makedirs(os.path.join(data, 'logs', d), exist_ok=True)
sent = []
m = live.Meter.__new__(live.Meter)
m.uid = '100000000'
m.submitter = types.SimpleNamespace(config={'screenshots': True})
m.beats_best = lambda s: ['dps'] if (s.get('battle_data') or {}).get('players') else ['score']
m.send_result = lambda payload, evidence, **files: sent.append((payload, evidence, files))
fight = types.SimpleNamespace(log=types.SimpleNamespace(name='20261113_120000_TestBoss.csv'))
path = m.run_path(fight)
shot = os.path.join('logs', 'screens', 'result_20261113_120300.jpg')
open(os.path.join(data, shot), 'wb').write(b'\xff\xd8\xff')

def write(s):
    with open(path, 'w', encoding='utf-8') as fh: json.dump(s, fh)
def read():
    with open(path, encoding='utf-8') as fh: return json.load(fh)

# the run goes without Battle Data: a copy of its result picture is kept for the late window
write({'boss': 'Test Boss', 'result': {'score': 12345, 'stars': 20, 'time': 180, 'screenshot': shot}})
m.decide_run(path)
s = read()
check('first send', len(sent), 1)
check('result copy kept', bool(s.get('late_result')) and os.path.isfile(os.path.join(data, s['late_result'])), True)
check('Battle Data can still follow', m.run_to_send(fight), True)

# nothing in Battle Data yet: nothing sent again
m.send_late_battle_data(fight)
check('no Battle Data: no second send', len(sent), 1)

# Battle Data opened late: the run goes again with it and the kept result picture, once
s['battle_data'] = {'players': [{'slot': 0, 'avg_dps': 50000}], 'screenshot': os.path.join('logs', 'battle_data', 'bd.jpg')}
write(s)
m.send_late_battle_data(fight)
check('late Battle Data sent', len(sent), 2)
if len(sent) == 2:
    payload, evidence, files = sent[1]
    check('with the kept result picture', evidence, s['late_result'])
    check('with the Battle Data picture', files.get('battle'), s['battle_data']['screenshot'])
check('its new bests joined', read().get('new_bests'), ['dps', 'score'])
m.send_late_battle_data(fight)
check('never a third copy', len(sent), 2)
check('Battle Data no longer kept as proof', m.run_to_send(fight), False)

# after the late window: nothing goes again, and the kept copy is swept
s = read(); s.pop('late_sent'); s['late_until'] = time.time() - 1; write(s)
m.send_late_battle_data(fight)
check('window over: not sent', len(sent), 2)
LB.LeaderboardsMixin.sweep_late_results(time.time() + LB.LATE_BATTLE_DATA_S + 61)
check('kept copy swept', os.path.isfile(os.path.join(data, s['late_result'])), False)

# a run sent with its Battle Data keeps no copy
write({'boss': 'Test Boss', 'result': {'score': 1, 'screenshot': shot}, 'battle_data': {'players': [{}], 'screenshot': 'logs/battle_data/x.jpg'}})
m.decide_run(path)
check('with Battle Data: no copy', read().get('late_result'), None)

if fails:
    print('FAIL\n  ' + '\n  '.join(fails)); sys.exit(1)
print('late_battle_data: all checks pass')
