"""A run still waiting for its Battle Data when the meter closes is sent, not dropped.

    python tests/waiting_run_on_quit.py

Meter.send_waiting_run: on 12 Nov a new best run waited the 45 s for its Battle Data (BATTLE_DATA_WAIT), aLogs was
quit 39 s in, and the run never went. Now closing the meter decides the waiting run on what it has (it lands in the
send queue on disk), once; nothing waiting means nothing sent. Made-up numbers only. Exit code 1 on any failure.
"""
import json, os, sys, tempfile
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_quit_')
import live

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

sent = []
m = live.Meter.__new__(live.Meter)
m.uid = '100000000'
m.beats_best = lambda s: ['score']
m.send_result = lambda payload, evidence, **files: sent.append(payload)

run = os.path.join(os.environ['ALOGS_DATA'], 'run.json')
with open(run, 'w', encoding='utf-8') as fh:
    json.dump({'boss': 'Test Boss', 'result': {'score': 12345, 'stars': 20, 'time': 180}}, fh)

m.pending_run = None
m.send_waiting_run()
check('nothing waiting: nothing sent', len(sent), 0)

m.pending_run = (run, 10 ** 12)                    # its 45 s are far from over
m.send_waiting_run()
check('waiting run sent on close', len(sent), 1)
check('nothing left waiting', m.pending_run, None)
with open(run, encoding='utf-8') as fh: s = json.load(fh)
check('marked decided and sent', (s.get('submit'), s.get('sent')), (True, True))

m.pending_run = (run, 10 ** 12)                    # the same run again (already decided): never a second copy
m.send_waiting_run()
check('a decided run is not sent twice', len(sent), 1)

if fails:
    print('FAIL\n  ' + '\n  '.join(fails)); sys.exit(1)
print('waiting_run_on_quit: all checks pass')
