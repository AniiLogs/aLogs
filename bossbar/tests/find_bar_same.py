"""find_bar with the bracket column shortcut (reader._bright_columns) finds exactly what it found without it: the same
bar (or None) and the same reason for none, and the row scan (reader._colored_run) the same run as the pixel walk, on every saved test picture (also upscaled 2x) and on made-up frames with
red/orange shapes and white rims and no bar (the slow case: 203 -> 30 ms at 4K, 14 Nov).

    python tests/find_bar_same.py

Exit code 1 on any difference.
"""
import glob, os, random, sys, time
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__))); sys.path.insert(0, HERE)
from PIL import Image, ImageDraw
import reader as R

fast, fast_run = R._bright_columns, R._colored_run
slow = lambda *a, **k: (lambda x: 1 << 30)            # never rules a column out: the check as it was
def slow_run(im, y, x0, x1): return R._longest_run(im.load(), y, x0, x1, R.colored)   # the pixel walk as it was
def both(im):
    R._bright_columns, R._colored_run = slow, slow_run
    t = time.perf_counter(); a = (R.find_bar(im), R.LAST_FAIL[0]); ta = time.perf_counter() - t
    R._bright_columns, R._colored_run = fast, fast_run
    t = time.perf_counter(); b = (R.find_bar(im), R.LAST_FAIL[0]); tb = time.perf_counter() - t
    return a, b, ta, tb

frames = []
for f in sorted(glob.glob(os.path.join(HERE, 'test_shots', '**', '*.*'), recursive=True)):
    if not f.lower().endswith(('.png', '.webp', '.jpg')): continue
    im = Image.open(f).convert('RGB')
    if im.width < 640: continue
    frames.append((os.path.relpath(f, HERE), im))
    if im.width < 2000: frames.append((os.path.relpath(f, HERE) + ' x2', im.resize((im.width * 2, im.height * 2), Image.NEAREST)))
random.seed(1)
for k in range(6):                                     # fire/portal-like shapes near the top, some with white rims
    im = Image.new('RGB', (3840, 2160), (40, 60, 80)); d = ImageDraw.Draw(im)
    for j in range(10):
        x0, y0 = random.randint(400, 2400), random.randint(60, 420); w, h = random.randint(300, 1200), random.randint(14, 40)
        d.rectangle((x0, y0, x0 + w, y0 + h), fill=(245, random.randint(95, 180), random.randint(20, 110)))
        if j % 2: d.rectangle((x0 - 4, y0 - 30, x0 - 2, y0 + h + 30), fill=(235, 235, 235))
        if j % 3 == 0: d.rectangle((x0 + w + 2, y0 - 25, x0 + w + 3, y0 + h + 25), fill=(200, 200, 200))
    frames.append((f'made-up red shapes {k}', im))

diff, found, ta_all, tb_all = [], 0, 0.0, 0.0
for name, im in frames:
    a, b, ta, tb = both(im); ta_all += ta; tb_all += tb
    if a != b: diff.append(f'{name}: without {a!r}, with {b!r}')
    found += a[0] is not None
    px = im.load(); rnd = random.Random(name)                # the row scan alone, on rows through the top of the frame
    for y in [rnd.randrange(0, max(1, im.height // 4)) for _ in range(40)]:
        x0 = rnd.randrange(0, im.width // 2)
        if fast_run(im, y, x0, im.width) != R._longest_run(px, y, x0, im.width, R.colored):
            diff.append(f'{name}: row {y} from {x0}: {fast_run(im, y, x0, im.width)} vs {R._longest_run(px, y, x0, im.width, R.colored)}')
print(f'{len(frames)} frames, {found} with a bar; find_bar {ta_all * 1000:.0f} ms -> {tb_all * 1000:.0f} ms in all')
if diff:
    print('FAIL\n  ' + '\n  '.join(diff)); sys.exit(1)
print('PASS')
