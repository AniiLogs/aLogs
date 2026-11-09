"""The HP labels are placed from a bar that can't shrink under them.

    python tests/hp_label_span.py

8 Nov: a bar first read short (sliding in, or under a flash) put a label over its real right end. The meter covers its own
labels in its pictures, so that hid the right bracket: "no bar" for 84% of a fight, the labels gone or frozen. Boss bars are drawn
centred, so the left bracket gives the whole span: a short read is widened to it, and a full or odd one is left alone.
Exit code 1 on any failure.
"""
import os, sys, tempfile
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_span_')
import reader as R

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

W = 3840
check('read short: the mirror of its left bracket', R.label_span(1168, 1179, W), 1504)
check('read right: as it was', R.label_span(1168, 1504, W), 1504)
check('a pixel or two off: as it was', R.label_span(1168, 1500, W), 1500)
check('longer than the mirror: as it was', R.label_span(1168, 1520, W), 1520)
check('a shape left of centre, nothing like a bar: as it was', R.label_span(200, 300, W), 300)
check('left bracket past the centre: as it was', R.label_span(2000, 300, W), 300)
if fails:
    print('\n'.join(fails)); sys.exit(1)
print('hp label span: ok')
