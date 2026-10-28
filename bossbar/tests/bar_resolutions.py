"""The boss HP bar at 720p, 1080p, 1440p and 4K.

    python tests/bar_resolutions.py [frames or folders...]

For each saved game frame where the meter finds a boss bar at its own resolution: find it again in the frame scaled
to each common resolution, and read its fill. Position and size must scale with the frame (within a few pixels)
and the fill must agree (within 1.5 points of 100), else the bar's HP would be read differently on that monitor. The
xN counter (Interlink's segments) must read the same too.
"""
import glob, os, sys
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
from PIL import Image
import reader as R, xn_digits as X
TPL = X.load_templates()

SIZES = {'720p': (1280, 720), '900p': (1600, 900), '1080p': (1920, 1080), '1440p': (2560, 1440), '4K': (3840, 2160)}

def frames(args):
    out = []
    for a in args:
        out += sorted(glob.glob(os.path.join(a, '**', '*.*'), recursive=True)) if os.path.isdir(a) else [a]
    return [p for p in out if p.lower().endswith(('.png', '.jpg', '.jpeg', '.webp'))]

def read(im):
    bar = R.find_bar(im)
    if not bar: return None
    fill = R.measure_fill(im, bar)
    fill = fill[0] if isinstance(fill, tuple) else fill
    fill = None if fill is None else fill * 100            # percent
    xn = X.read_counter(im.crop(tuple(int(v) for v in R.box_at(bar, R.XN_BOX))), TPL)
    return {'x': bar['left_bracket'] / im.width, 'y': bar['yc'] / im.height, 'span': bar['span'] / im.width, 'fill': fill, 'xn': xn}

if __name__ == '__main__':
    checked = bad = 0
    for path in frames(sys.argv[1:]):
        src = Image.open(path).convert('RGB')
        base = read(src)
        if not base: continue
        checked += 1
        line = []
        for name, size in SIZES.items():
            got = read(src.resize(size, Image.LANCZOS) if src.size != size else src)
            if not got:
                line.append(f'{name}: NOT FOUND'); bad += 1; continue
            off = max(abs(got['x'] - base['x']) * size[0], abs(got['y'] - base['y']) * size[1], abs(got['span'] - base['span']) * size[0])
            dfill = None if got['fill'] is None or base['fill'] is None else abs(got['fill'] - base['fill'])
            ok = off <= 4 and (dfill is None or dfill <= 1.5) and got['xn'] == base['xn']
            bad += not ok
            line.append(f'{name}: {"ok" if ok else "OFF"} (pos {off:.1f}px, fill {got["fill"] if got["fill"] is None else round(got["fill"], 1)}, xN {got["xn"]})')
        print(f'{os.path.relpath(path)[-60:]}  fill {base["fill"]}: ' + ' | '.join(line))
    print(f'\n{checked} frames with a bar, {bad} problems')
    sys.exit(1 if bad else 0)
