"""Sample pictures (logs/samples: the Party screen, prep, an Interlink fight's HUD, after an Egg Heist match) are
test pictures: kept only while Captures & logs › Save test pictures (capture.test_frames) is on (14 Nov, the owner's
call).

    python tests/sample_pictures.py

- off (or no settings at all): nothing written, not even the folder, and the gap isn't used up;
- on: the picture is written, as before;
- every caller in the meter passes the live settings (live_cfg: self.args.cfg, replaced by reload_settings).
Made-up pictures in a temp data folder (never the real logs). Exit code 1 on any failure.
"""
import logging, os, re, sys, tempfile
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
TMP = tempfile.mkdtemp(prefix='alogs_samples_')
os.environ['ALOGS_DATA'] = TMP
os.environ.pop('ALOGS_DEV_ON', None)
from PIL import Image
import core
logging.getLogger('meter').addHandler(logging.NullHandler()); logging.getLogger('meter').propagate = False

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

SAMPLES = os.path.join(TMP, 'logs', 'samples')
def files(): return sorted(os.listdir(SAMPLES)) if os.path.isdir(SAMPLES) else []
check('the data folder is the temp one', os.path.normcase(core.HERE), os.path.normcase(TMP))

im = Image.new('RGB', (320, 180), (40, 80, 120))
OFF = {'capture': {'test_frames': False}}; ON = {'capture': {'test_frames': True}}
core.keep_sample(im, 'party', OFF)
core.keep_sample(im, 'prep', None)
core.keep_sample(im, 'egg_after', {}, gap=3, kept=15)
check('off: nothing written', files(), [])
check('off: not even the folder', os.path.isdir(SAMPLES), False)
check('off: the gap is not used up', sorted(core._sample_at), [])

core.keep_sample(im, 'party', ON); core.WRITER.flush()
check('on: the picture is written', [re.sub(r'\d', '0', n) for n in files()], ['party_00000000_000000.png'])
core.keep_sample(im, 'egg_after', ON, gap=0); core.WRITER.flush()
check('on: a run of pictures as JPEG', sorted(n.split('_')[0] + n[-4:] for n in files()), ['egg.jpg', 'party.png'])
core.keep_sample(im, 'party', OFF); core.WRITER.flush()
check('turned off again: nothing more', len(files()), 2)
Meter = type('M', (), {})
m = Meter(); check('a meter without settings: none', core.live_cfg(m), None)
m.args = type('A', (), {'cfg': ON})(); check('the live settings', core.live_cfg(m) is ON, True)
m.args.cfg = OFF; check('...as reload_settings replaced them', core.live_cfg(m) is OFF, True)

for name in ('live.py', 'fights.py'):
    with open(os.path.join(HERE, name), encoding='utf-8') as f: src = f.read()
    calls = re.findall(r'keep_sample\(([^)]*)\)', src)
    check(f'{name}: every call passes the live settings', [c for c in calls if 'live_cfg(self' not in c], [])

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('sample pictures: ok')
