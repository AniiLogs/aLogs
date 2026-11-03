"""Windows OCR's language, from the OCR helper to the app: the helper's first line (READY<TAB><tag>), reader.Ocr's
language, the meter's log line at start and status.json's ocr, with this PC's en-US OCR and as a PC without English
OCR has it.

    python tests/ocr_language.py

The helper is the real one (bossbar/ocr-helper.exe, built by: cd app && npm run ocr:build), started with
ALOGS_OCR_TEST_LANGS, its test-only list of the OCR languages to act as if they were the only ones installed:
- en-US: READY<TAB>en-US, and a boss title drawn in Arial reads as it is (parse_title: Lv 60, Alpha Glameep);
- empty (none): READY<TAB>, every Windows OCR line empty, while the Latin and Korean name models still read;
- ko-KR, which this PC doesn't have: as none, its user-profile fallback included (that one would find en-US);
- not set, as aLogs starts it: en-US on this PC.
Python on Windows drops a variable set to '' from what a child process gets, so the helper started directly gets an
empty value in its own environment, and reader.Ocr and the meter (which start it themselves) get ',': a list with no
tag, which the helper also reads as none. Then the meter on each: Meter()'s log line and status.json's ocr
(write_status); the same on a helper that doesn't start (NoOcr), on one that doesn't say (a plain READY) and on one
that says another language (ko-KR: this PC's helper can't, so a stand-in says it); and the PowerShell helper
(ocr_win.ps1, used when there is no ocr-helper.exe), which says en-US. Last, English added while the meter runs: a
helper started later (the HP reader's) leaves status.json saying the meter's own has none, and ocr_recheck restarts
every helper the meter has once a fresh one reads English. The drawn names are made up. Needs a PC with en-US
Windows OCR.

Nothing is sent and nothing lands in your logs: the meter's data folder is a temporary one. Exit code 1 on any failure.
"""
import logging, os, shutil, struct, subprocess, sys, tempfile, time, types
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
TMP = tempfile.mkdtemp(prefix='alogs_ocrlang_')
os.environ['ALOGS_DATA'] = TMP                  # before settings is imported: a fresh data folder
os.environ.pop('ALOGS_OCR_TEST_LANGS', None)
from PIL import Image, ImageDraw, ImageFont
import live, reader as R, hangulnames as K, settings as C

class LogList(logging.Handler):
    """The meter's log lines (level, text)."""
    def __init__(self): super().__init__(); self.lines = []
    def emit(self, record): self.lines.append((record.levelname, record.getMessage()))
LOG = LogList()
logging.getLogger('meter').addHandler(LOG); logging.getLogger('meter').setLevel(logging.INFO)

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

def draw(text, font, size=40, pad=16):
    """`text` in black on white, as the meter's prepared boxes are."""
    f = ImageFont.truetype(font, size); l, t, r, b = f.getbbox(text)
    im = Image.new('L', (r - l + 2 * pad, b - t + 2 * pad), 255)
    ImageDraw.Draw(im).text((pad - l, pad - t), text, fill=0, font=f)
    return im

TITLE, LATIN, HANGUL = 'Lv. 60 Alpha Glameep', 'Moonfox_Rin42', '달빛여우'      # as the bar shows a title
BOXES = [(0, draw(TITLE, 'arial.ttf')),         # Windows OCR alone
         (4, draw(LATIN, 'arial.ttf')),         # Windows OCR and the Latin model
         (6, draw(LATIN, 'arial.ttf')),         # the Latin model alone (flag 2: no Windows OCR)
         (3, draw(HANGUL, 'malgun.ttf'))]       # the Korean model alone

def with_langs(langs):
    """The environment a helper that reader.Ocr starts gets: ALOGS_OCR_TEST_LANGS set to `langs`, or not set (None).
    Not '': Windows would drop it (use ',')."""
    assert langs != '', "an empty value doesn't reach the helper: use ','"
    if langs is None: os.environ.pop('ALOGS_OCR_TEST_LANGS', None)
    else: os.environ['ALOGS_OCR_TEST_LANGS'] = langs

def helper_reads(langs):
    """The helper's first line, and its answers to BOXES (one OCR2 request): (Windows OCR, Korean letters' best
    characters, Latin model) each. Its own environment, so an empty value reaches it."""
    env = {k: v for k, v in os.environ.items() if k != 'ALOGS_OCR_TEST_LANGS'}
    if langs is not None: env['ALOGS_OCR_TEST_LANGS'] = langs
    p = subprocess.Popen([R.OCR_EXE], stdin=subprocess.PIPE, stdout=subprocess.PIPE, env=env,
                         creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
    try:
        first = p.stdout.readline().decode('utf-8')
        parts = [b'OCR2', struct.pack('<I', len(BOXES))]
        for flags, im in BOXES: parts += [bytes([flags]), struct.pack('<II', *im.size), im.tobytes()]
        p.stdin.write(b''.join(parts)); p.stdin.flush()
        reads = []
        for _ in BOXES:
            text, letters, latin = K.split_reply(p.stdout.readline().decode('utf-8').rstrip('\r\n'))
            reads.append((text, ''.join(picks[0][0] for _, picks in letters) if letters else '', latin))
        return first, reads
    finally:
        p.stdin.close(); p.wait(10)

def meter(make_ocr=None):
    """Meter() as the meter starts it (its OCR helper on the environment set), with what it logged and the status it
    writes: (meter, log lines, status)."""
    real_ocr = R.Ocr
    if make_ocr: R.Ocr = make_ocr
    LOG.lines = []
    try:
        cfg = C.load()
        args = types.SimpleNamespace(cfg=cfg, replay=['ocr language'], interval=0.05, title='Aniimo', duration=180,
                                     level=None, boss_id=None, player_log=os.path.join(TMP, 'Player.log'), no_overlay=True,
                                     hide_from_capture=False, no_debug_frames=True, no_auto_click=True, no_submit=True)
        m = live.Meter(args, lambda kind, data: None)
    finally: R.Ocr = real_ocr
    status = []
    real_status = C.write_status; C.write_status = status.append
    try: m.write_status(time.time())
    finally: C.write_status = real_status
    return m, list(LOG.lines), status[-1] if status else {}

def ocr_lines(lines): return [(lvl, t) for lvl, t in lines if t.startswith('Windows OCR')]

live.load_korean_names = lambda *a, **k: None     # the game's fonts: not needed here
print(f'helper: {R.OCR_EXE}')
if not os.path.exists(R.OCR_EXE): print('no ocr-helper.exe: build it (cd app && npm run ocr:build)'); sys.exit(1)

# ---- the helper itself
first, en = helper_reads('en-US')
check('en-US: first line', first, 'READY\ten-US\n')
check('en-US: the title, Windows OCR', en[0][0], TITLE)
check('en-US: the title parsed', R.parse_title(en[0][0]), (60, 'Alpha Glameep'))
check('en-US: the name, Latin model', en[1][2], LATIN)
check('en-US: the name, Latin model alone', en[2], ('', '', LATIN))
check('en-US: the Hangul name, Korean model alone', en[3], ('', HANGUL, ''))
for langs, why in (('', 'none'), (',', 'none, as a list with no tag'), ('ko-KR', 'ko-KR (not on this PC)')):
    first, reads = helper_reads(langs)
    check(f'{why}: first line', first, 'READY\t\n')
    check(f'{why}: every Windows OCR line', [r[0] for r in reads], ['', '', '', ''])
    # the name models read as they do with English OCR
    check(f'{why}: the name models', [r[1:] for r in reads], [r[1:] for r in en])
first, reads = helper_reads(None)
check('not set: first line', first, 'READY\ten-US\n')
check('not set: the reads', reads, en)
print(f'helper: en-US reads {en[0][0]!r}, {en[1][2]!r}, {en[3][1]!r}; none and ko-KR read the names alone')

# ---- reader.Ocr
for langs, want in (('en-US', 'en-US'), (',', ''), ('ko-KR', '')):
    with_langs(langs)
    ocr = R.Ocr()
    try:
        # on the reader, not the class: a helper started later can pick another (below)
        check(f'Ocr.language with {langs!r}', (ocr.language, R.Ocr.language), (want, None))
        check(f'Ocr.read (OCR1) with {langs!r}', ocr.read({'t': BOXES[0][1]}), {'t': TITLE if want else ''})
    finally: ocr.close()
check('ocr_english', [R.ocr_english(v) for v in ('en-US', 'en-GB', 'EN', 'ko-KR', '', None)], [True, True, True, False, False, None])
check('ready_language', [R.ready_language(v) for v in ('READY\ten-US\n', 'READY\tko-KR\r\n', 'READY\t\n', 'READY\r\n')],
      ['en-US', 'ko-KR', '', None])
try: R.ready_language('error\n'); fails.append('ready_language: a line that is not READY did not raise')
except RuntimeError: pass
# the PowerShell helper (no ocr-helper.exe) says its language the same way
with_langs(None)
real_exe, R.OCR_EXE = R.OCR_EXE, os.path.join(TMP, 'no-ocr-helper.exe')
try:
    ocr = R.Ocr()
    try:
        check('PowerShell helper: native', ocr.native, False)
        check('PowerShell helper: Ocr.language', ocr.language, 'en-US')
        check('PowerShell helper: the title', ocr.read({'t': BOXES[0][1]}), {'t': TITLE})
    finally: ocr.close()
finally: R.OCR_EXE = real_exe
print(f'reader.Ocr: en-US, none and ko-KR as the helper says; PowerShell helper en-US')

# ---- the meter: its log line at start and status.json
NOT_READ = "boss names and levels, result screens, Battle Data, the battle prep and Party screens can't be read"
keys = None
for langs, want, level, start in (
        ('en-US', {'language': 'en-US', 'english': True, 'started': True}, 'INFO', 'Windows OCR: en-US'),
        (',', {'language': None, 'english': False, 'started': True}, 'WARNING', 'Windows OCR: none (no OCR language on this PC)'),
        ('ko-KR', {'language': None, 'english': False, 'started': True}, 'WARNING', 'Windows OCR: none (no OCR language on this PC)')):
    with_langs(langs)
    m, lines, status = meter()
    try:
        check(f'meter on {langs!r}: status ocr', status.get('ocr'), want)
        said = ocr_lines(lines)
        check(f'meter on {langs!r}: one Windows OCR line', len(said), 1)
        if said:
            check(f'meter on {langs!r}: its level', said[0][0], level)
            check(f'meter on {langs!r}: its start', said[0][1][:len(start)], start)
            if level == 'WARNING':
                check(f"meter on {langs!r}: it says what isn't read", NOT_READ in said[0][1], True)
                check(f"meter on {langs!r}: and that runs aren't recorded", "runs aren't recorded or sent" in said[0][1], True)
            print(f'meter on {langs!r}: {said[0][0]} {said[0][1]}')
        # status.json keeps every field it had, and adds ocr
        if keys is None: keys = set(status)
        check(f'meter on {langs!r}: status fields', set(status), keys)
    finally: m.ocr.close()
for f in ('time', 'pid', 'status', 'game_window', 'capture', 'homeland', 'frame', 'boss_bar', 'fight', 'character',
          'auto_battle_data', 'leaderboard', 'ocr'):
    check(f'status field {f}', f in (keys or ()), True)
# a helper that doesn't start: NoOcr, which counts as no OCR
def no_helper(*a, **k): raise RuntimeError('Windows OCR helper did not start')
m, lines, status = meter(no_helper)
check('no helper: Meter.ocr', type(m.ocr).__name__, 'NoOcr')
check('no helper: status ocr', status.get('ocr'), {'language': None, 'english': False, 'started': False})
said = ocr_lines(lines)
check('no helper: its line', [(lvl, t[:len("Windows OCR: none (its helper didn't start)")]) for lvl, t in said],
      [('WARNING', "Windows OCR: none (its helper didn't start)")])
check("no helper: it says what isn't read", bool(said) and NOT_READ in said[0][1], True)
print(f'no helper: {said[0][0]} {said[0][1]}' if said else 'no helper: no line')
# a helper that doesn't say (a plain READY: an older ocr-helper.exe): not known, so no warning
class Unsaid(R.NoOcr):
    started = True; language = None
m, lines, status = meter(Unsaid)
check("not said: status ocr", status.get('ocr'), {'language': None, 'english': None, 'started': True})
check("not said: its line", [lvl for lvl, _ in ocr_lines(lines)], ['INFO'])
# another language, as the helper's user-profile fallback gives on a Korean PC with no English OCR (this PC can't make
# its helper say one: it has en-US alone)
class Korean(R.NoOcr):
    started = True; language = 'ko-KR'
m, lines, status = meter(Korean)
check('ko-KR: status ocr', status.get('ocr'), {'language': 'ko-KR', 'english': False, 'started': True})
said = ocr_lines(lines)
check('ko-KR: its line', [(lvl, t[:len('Windows OCR: ko-KR, not English')]) for lvl, t in said],
      [('WARNING', 'Windows OCR: ko-KR, not English')])
check("ko-KR: it says what isn't read", bool(said) and NOT_READ in said[0][1], True)
print(f'ko-KR: {said[0][0]} {said[0][1]}' if said else 'ko-KR: no line')

# ---- English added while the meter runs (Windows Settings): a helper started later doesn't change what status.json
# says about the meter's own (hit_watch starts the HP reader's at the first tab-out, hours on), and ocr_recheck finds
# English and restarts every helper the meter has, so the game's screens are read without restarting aLogs
def status_now(m):
    """The status m writes now (write_status waits a second between writes)."""
    got = []; real = C.write_status; C.write_status = got.append
    try: m.last_status = 0; m.write_status(time.time())
    finally: C.write_status = real
    return got[-1] if got else {}
with_langs(',')
m, lines, status = meter()
workers = [live.OcrWorker(), live.OcrWorker()]          # the bar's and the screens' (not started in replay mode)
for w in workers: w.start()
deadline = time.time() + 10
while any(w.ocr is None for w in workers) and time.time() < deadline: time.sleep(0.01)
m.ocr_bg, m.screen_bg = workers
m.hp_ocr = R.Ocr()
with_langs('en-US')
later = R.Ocr()                                         # as hit_watch's, started after English was added
try:
    check('English added: a later helper says', later.language, 'en-US')
    check("English added: a later helper doesn't change status ocr", status_now(m).get('ocr'),
          {'language': None, 'english': False, 'started': True})
    check("English added: the meter's own helper still reads nothing", m.ocr.read({'t': BOXES[0][1]}), {'t': ''})
finally: later.close()
recheck = getattr(m, 'ocr_recheck', None)
check('Meter.ocr_recheck', callable(recheck), True)
if callable(recheck):
    LOG.lines = []
    recheck(time.time())                                # within OCR_RECHECK of the start: no look yet
    check('no look at start', getattr(m, 'ocr_probe', None), None)
    with_langs(',')                                     # still none: nothing restarts
    m.ocr_checked -= live.OCR_RECHECK
    recheck(time.time()); m.ocr_probe.join(10)
    check('still none: no restart', [o.language for o in (m.ocr, m.ocr_bg.ocr, m.screen_bg.ocr, m.hp_ocr)], ['', '', '', ''])
    check('still none: no line', ocr_lines(LOG.lines), [])
    with_langs('en-US')
    recheck(time.time())                                # within OCR_RECHECK of the last look: not yet
    m.ocr_probe.join(10)
    check('rechecks wait OCR_RECHECK', m.ocr.language, '')
    m.ocr_checked -= live.OCR_RECHECK
    recheck(time.time()); m.ocr_probe.join(10)
    check('English found: every helper restarted', [o.language for o in (m.ocr, m.ocr_bg.ocr, m.screen_bg.ocr, m.hp_ocr)],
          ['en-US'] * 4)
    check("English found: the meter's own helper reads", m.ocr.read({'t': BOXES[0][1]}), {'t': TITLE})
    m.ocr_bg.submit({'t': BOXES[0][1]}, 'bar'); deadline = time.time() + 10
    while m.ocr_bg.result is None and time.time() < deadline: time.sleep(0.01)
    check("English found: the bar's helper reads", m.ocr_bg.take(), ({'t': TITLE}, 'bar'))
    said = ocr_lines(LOG.lines)
    check('English found: one line', [(lvl, t[:len('Windows OCR: en-US now')]) for lvl, t in said],
          [('INFO', 'Windows OCR: en-US now')])
    print(f'English found: {said[0][0]} {said[0][1]}' if said else 'English found: no line')
    check('English found: status ocr', status_now(m).get('ocr'), {'language': 'en-US', 'english': True, 'started': True})
    probe = m.ocr_probe; m.ocr_checked -= live.OCR_RECHECK       # English now: no more looks
    recheck(time.time())
    check('English: no more rechecks', m.ocr_probe is probe, True)
for w in workers: w.close()
m.hp_ocr.close(); m.ocr.close()
# a helper that didn't start (NoOcr) isn't rechecked: installing OCR doesn't start it
m, lines, status = meter(no_helper)
if callable(getattr(m, 'ocr_recheck', None)):
    m.ocr_recheck(time.time())
    check('no helper: no recheck', getattr(m, 'ocr_probe', None), None)
with_langs(None)

shutil.rmtree(TMP, ignore_errors=True)
if fails:
    print(f'{len(fails)} FAILED:'); [print('  ' + f) for f in fails]; sys.exit(1)
print('Windows OCR language: helper, reader.Ocr, meter log and status all right')
