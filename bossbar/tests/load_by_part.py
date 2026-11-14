"""General › Performance › "What each part of aLogs uses": the meter.log load line splits aLogs' CPU by part (14 Nov: a lag
report had the whole PC 15 points busier on B9 than on B7 with every part it measured as light as before; the WebView2
processes, Sanctum Helper's CPU and a leftover updating window were counted nowhere).

    python tests/load_by_part.py

- pcstats.alogs_groups sorts a process list into meter (with its OCR helpers), app, its windows (WebView2), Sanctum
  Helper and left over (an updating / installing window's copy, a second aLogs app); a process whose parent's pid
  went to a newer program isn't that program's child; other programs are in no group.
- CpuLoad.groups: nothing on the first call; then what each group used since, a process started since counted whole,
  a pid used again by a new process not counted from the old one's time.
- The real load line: switched off, it's as before (no "by part"); switched on, it names the parts and "other
  programs", never another program's name.
Exit code 1 on any failure.
"""
import os, sys, logging
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__))); sys.path.insert(0, HERE)
import pcstats
from core import CpuLoad

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

# -- the groups ------------------------------------------------------------------------------------------------------
T = {1: (0, 'explorer.exe'), 100: (1, 'alogs-app.exe'), 200: (100, 'meter.exe'), 201: (200, 'meter.exe'),
     300: (100, 'msedgewebview2.exe'), 301: (300, 'msedgewebview2.exe'), 302: (300, 'msedgewebview2.exe'),
     400: (100, 'sanctumhelper.exe'), 401: (400, 'conhost.exe'), 150: (100, 'rewasdcommandline.exe'),
     600: (100, 'chrome.exe'),                                   # pid 100's earlier owner started it: not ours
     700: (1, 'alogs-updating.exe'), 701: (700, 'msedgewebview2.exe'), 800: (1, 'alogs-app.exe'),
     900: (1, 'game.exe'), 950: (2, 'ocr.exe')}
START = {100: 50.0, 600: 10.0}
g = pcstats.alogs_groups(T, 100, 200, 400, helpers=[950], started=lambda p: START.get(p, 60.0))
check('meter and its helpers', g['meter'], {200, 201, 950})
check('the app and what else it started', g['app'], {100, 150})
check('its windows', g['windows'], {300, 301, 302})
check('Sanctum Helper', g['sanctum'], {400, 401})
check('left over: the updating copy and a second app', g['leftover'], {700, 701, 800})
check('other programs in no group', {1, 600, 900} & set().union(*g.values()), set())
check('no app (the meter run on its own)', pcstats.alogs_groups(T, 0, 200)['app'], set())
real = pcstats.process_table()
check('this PC: the test itself is found as the meter', os.getpid() in pcstats.alogs_groups(real, 0, os.getpid())['meter'], True)

# -- CPU by group ----------------------------------------------------------------------------------------------------
c = CpuLoad(); state = {}
c.times = lambda p: state.get(p)
state.update({1: (5.0, 10.0), 2: (5.0, 1.0)})
c.span = 100.0
check('the first call: nothing yet', c.groups({'a': [1], 'b': [2]}), {'a': None, 'b': None})
c.pid_wall = 20.0
state.update({1: (5.0, 14.0), 2: (30.0, 3.0), 3: (25.0, 2.0)})   # 2: a new process on the old pid; 3: started since
check('since the last call', c.groups({'a': [1], 'b': [2, 3]}), {'a': 4.0, 'b': 5.0})
c.pid_wall = 40.0; state[3] = (25.0, 2.5)
check('a process started earlier, seen for the first time: not counted whole', c.groups({'a': [3, 9]}), {'a': 0.5})

# -- the real line ---------------------------------------------------------------------------------------------------
import live as L
lines = []
class Grab(logging.Handler):
    def emit(self, r): lines.append(r.getMessage())
logging.getLogger('meter').addHandler(Grab()); logging.getLogger('meter').setLevel(logging.INFO)
m = L.Meter.__new__(L.Meter)
class Args: cfg = {'app': {'load_details': False}}; replay = None
m.args = Args(); m.hwnd = None; m.ocr = m.ocr_bg = m.screen_bg = None; m.space = None; m.fight = None
m.bossrush_space = m.holosim_space = None; m.idle = lambda: True
for i, on in enumerate((False, False, True, True)):
    Args.cfg = {'app': {'load_details': on}}; lines.clear(); m.load_line(1000 + 60 * i)
    line = next((l for l in lines if l.startswith('load:')), '')
    check(f'switched {"on" if on else "off"}: a load line', line.startswith('load: CPU meter'), True)
    check(f'switched {"on" if on else "off"}: by part', 'aLogs by part: meter' in line, on)
    if on: check('other programs as one share', '; other programs ' in line, True)
check('no other program named', any(n in ' '.join(lines) for n in ('explorer', 'chrome', '.exe')), False)
print(lines[-1][:400])

if fails:
    print('FAIL\n  ' + '\n  '.join(fails)); sys.exit(1)
print('PASS')
