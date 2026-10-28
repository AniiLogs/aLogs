"""Every screen reader at the common monitor resolutions: 720p, 1080p, 1440p and 4K.

    python tests/resolutions.py [sample images or folders...]

Each sample (a real screenshot) goes through the meter's own screen dispatch (check_battle_data) once at its own
resolution and once scaled to each of the others. What each reader got (the parsed part of its log line, after
'->') must be the same everywhere. Nothing is sent and nothing lands in your logs: the meter writes into a temporary
folder, the leaderboard sender is off and nothing is clicked.
"""
import atexit, glob, logging, os, re, shutil, sys, tempfile, time, types
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
from PIL import Image
import live, reader as R

SIZES = {'720p': (1280, 720), '900p': (1600, 900), '1080p': (1920, 1080), '1440p': (2560, 1440), '4K': (3840, 2160)}
TMP = tempfile.mkdtemp(prefix='alogs_res_')
atexit.register(shutil.rmtree, TMP, ignore_errors=True)   # 41 of these were left in %TEMP%
live.HERE = TMP; os.makedirs(os.path.join(TMP, 'logs'), exist_ok=True)

class Capture(logging.Handler):
    def __init__(self): super().__init__(); self.lines = []
    def emit(self, record): self.lines.append(record.getMessage())
cap = Capture(); logging.getLogger('meter').addHandler(cap); logging.getLogger('meter').setLevel(logging.INFO)

METER = next(c for c in vars(live).values() if isinstance(c, type) and hasattr(c, 'check_battle_data'))
OCR = R.Ocr()

def meter():
    """A meter with just the state the screen readers use: no game, no window, no sending, no clicking."""
    m = METER.__new__(METER)
    cfg = {'capture': {'keep_hours': 0, 'end_screens': False, 'debug_frames': False, 'nobar_frames': False},
           'results': {'auto_open_battle_data': False, 'hold_inputs': False, 'hold_controller': False, 'hold_seconds': 60},
           'leaderboard': {'submit': False, 'include_screenshots': False}}
    m.args = types.SimpleNamespace(cfg=cfg, replay=0, no_auto_click=True, duration=180, no_debug_frames=True, level=None, boss_id=None)
    m.ocr = OCR; m.catalog = live.R.load_catalog(); m.submitter = None; m.uid = '302000000000'; m.character_name = None
    for name in ('last_results', 'result_seen', 'stats_seen', 'catch_seen', 'battle_saved', 'hold_started', 'last_click',
                 'auto_until', 'egg_seen'):
        setattr(m, name, 0.0)
    m.clicks = 0; m.fight = None; m.last_fight = None; m.awaiting = None; m.after_fight = None; m.hwnd = None
    m.end_sig = None; m.party = None; m.prep_boss = None; m.space = None; m.last_scores = {}; m.origin = (0, 0)
    m.menu_proven = None; m.rush_level = None; m.bossrush_space = False; m.rush_spaces = set()
    m.hold = types.SimpleNamespace(start=lambda *a: None, stop=lambda: None, active=False)
    m.out = lambda *a, **k: None
    m.picture = lambda *a, **k: None
    return m

def readings(lines):
    """What each reader parsed: 'screen: ... -> value' lines, keeping only the screen name and the value."""
    out = []
    for line in lines:
        if '->' not in line: continue
        head, value = line.split('->', 1)
        out.append((head.split(':')[0].strip(), re.sub(r'\s+', ' ', value.strip())))
    return out

# the week's bosses as they were when the shot was taken (the game's schedule, pinned to the shot's date)
CYCLES = os.path.join(os.environ.get('LOCALAPPDATA', ''), 'aLogs', 'meter', 'boss_rush_cycles.json')
if os.path.exists(CYCLES): R.CYCLES_FILE[0] = CYCLES

def taken_at(path):
    m = re.search(r'(\d{8})_(\d{6})', os.path.basename(path))
    return time.mktime(time.strptime(m.group(1) + m.group(2), '%Y%m%d%H%M%S')) if m else time.time()

def run(im, when):
    m = meter(); cap.lines.clear()
    real = time.time
    time.time = lambda: when                     # the schedule lookups ask for "now"
    try: m.check_battle_data(im, when)
    finally: time.time = real
    return readings(cap.lines)

def samples(args):
    paths = []
    for a in args:
        paths += sorted(glob.glob(os.path.join(a, '**', '*.*'), recursive=True)) if os.path.isdir(a) else [a]   # test_frames/<day>/<kind>/
    return [p for p in paths if p.lower().endswith(('.png', '.jpg', '.jpeg', '.webp'))]

if __name__ == '__main__':
    failures = 0
    for path in samples(sys.argv[1:]):
        src = Image.open(path).convert('RGB')
        when = taken_at(path)
        results = {name: run(src.resize(size, Image.LANCZOS) if src.size != size else src, when) for name, size in SIZES.items()}
        native = next((n for n, s in SIZES.items() if s == src.size), None)
        base = results[native] if native else results['4K']
        print(f'\n{os.path.basename(path)} ({src.size[0]}x{src.size[1]})')
        if not base: print('   no reader recognised it at its own resolution'); continue
        for screen, value in base: print(f'   {screen}: {value[:150]}')
        for name, got in results.items():
            if got != base:
                failures += 1
                print(f'   ** {name} differs:')
                for (s1, v1), (s2, v2) in zip(base + [('', '')] * 9, got + [('', '')] * 9):
                    if (s1, v1) != (s2, v2):       # the part that differs, with a little context
                        i = next((n for n, (x, y) in enumerate(zip(v1, v2)) if x != y), min(len(v1), len(v2)))
                        print(f'      {s1 or s2}:\n         expected ...{v1[max(0, i - 25):i + 45]!r}\n         got      ...{v2[max(0, i - 25):i + 45]!r}')
                if len(got) != len(base): print(f'      ({len(base)} readings expected, {len(got)} got)')
    OCR.close()
    print(f'\n{"all resolutions agree" if not failures else f"{failures} resolution mismatches"}')
    sys.exit(1 if failures else 0)
