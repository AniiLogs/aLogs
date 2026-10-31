"""Boss-bar reading shared by bossbar.py (screenshots) and live.py (live capture).

Resolution-independent: the white [ ] brackets at the ends of the boss bar are the ruler. Every position is a
fraction of the bracket span, measured once on a 2000x1125 screenshot (span 789 px) and checked on a 4K crop.
"""
import csv, difflib, json, os, re, struct, subprocess, tempfile, threading, time
from PIL import Image, ImageChops, ImageOps
from hpcalc import max_hp, break_points

import settings as C
HERE = C.RES                      # shipped data files (catalog, OCR helper)

# ---- layout, in units of the bracket span (left bracket = 0, right bracket = 1) ----
REF_SPAN = 789                                   # bracket span on the 2000x1125 reference shot
TRACK = (9 / 789, 716 / 789)                     # fill track: left edge, right edge (exclusive)
WORLD_TRACK_RIGHT = 784 / 789                     # bosses out in the world have no xN box: the track runs to the bracket
# x0, y0, x1, y1; y relative to the fill's centre row. The title is centred over the bar, so a long one grows both ways:
# open-world bars are narrower than Interlink's (1293 px at 4K against 1514) with text the same size, and
# "Lv.60 Sparkling- Young Omega Sherro" filled almost the whole span - the old 175..625 box cut "Lv.60" off one end and
# "herro" off the other, and with no level no fight started. Now the full span plus a little: what OCR finds left of
# "Lv" is dropped by parse_title, and a bracket top read as ']' or '|' after the name is no letter to norm()
TITLE_BOX = (-30 / 789, -52 / 789, 819 / 789, -8 / 789)
XN_BOX = (722 / 789, -12 / 789, 783 / 789, 14 / 789)    # inside the dark xN box only
SPAN_PER_THICKNESS = 789 / 12                     # fallback when brackets aren't visible (fill is 12 px thick at span 789)

# ---- pixel classes ----
def is_fill(p): r, g, b = p[:3]; return r >= 220 and 90 <= g <= 195 and b <= 125        # live HP gradient
def is_ghost(p): r, g, b = p[:3]; return 165 <= r <= 215 and 55 <= g <= 105 and 55 <= b <= 100  # recent damage
def is_flash(p): return min(p[:3]) >= 235                                                # hit flash
def is_white(p): return min(p[:3]) >= 190                                                # brackets / text
def barlike(p): return is_fill(p) or is_ghost(p) or is_flash(p)
def colored(p): return is_fill(p) or is_ghost(p)                                         # no white: walls/sky are white

def _bar_run(px, y, x0, x1, gap):
    """Longest run of fill/ghost pixels on row y, allowing short white gaps (the hit flash)."""
    best = (0, 0); start = None; last = None
    for x in range(x0, x1):
        p = px[x, y]
        if colored(p):
            if start is None or x - last > gap: start = x
            last = x
            if last - start + 1 > best[0]: best = (last - start + 1, start)
        elif not is_flash(p) and start is not None and x - last > 0:
            start = None
    return best

def _is_bracket(px, x, yc, thick, W, H):
    """A bracket is a thin, tall white line: white along the column, not white a few pixels to either side. It is
    ~1.6x the bar's thickness; at small sizes (720p) blending adds a row to the measured thickness and takes the
    bracket's faded ends off, so 1.5 px of that is allowed for (was a plain 1.3x: missed at 720p)."""
    if _white_run(px, x, yc, 2 * thick, H) < 1.3 * thick - 1.5: return False
    off = max(3, thick // 3)
    side = [px[min(W - 1, max(0, x + s)), yy] for s in (-off, off) for yy in (yc - thick // 3, yc + thick // 3)]
    return sum(map(is_white, side)) <= 2

def _longest_run(px, y, x0, x1, test):
    best = (0, 0); run = start = 0
    for x in range(x0, x1):
        if test(px[x, y]):
            if run == 0: start = x
            run += 1
            if run > best[0]: best = (run, start)
        else: run = 0
    return best

def _line_px(px, x, y):
    """A pixel of a thin white line: white, or (scaled down, the 1 px line falling between two columns) light grey
    next to light grey - the line split over x and a neighbour, e.g. a bracket at 720p."""
    p = px[x, y]
    if is_white(p): return True
    if min(p[:3]) < 140: return False
    for nx in (x - 1, x + 1):
        try:
            if min(px[nx, y][:3]) >= 140: return True
        except IndexError:
            pass
    return False

def _white_run(px, x, yc, reach, H):
    """Length of the vertical white run through column x around row yc."""
    n = 0
    for y in range(max(0, yc - reach), min(H, yc + reach)):
        n = n + 1 if _line_px(px, x, y) else n
    return n

LAST_FAIL = ['']     # why the last find_bar call found nothing (for the meter's diagnostics)

def _band(lo, hi): return lambda v: 255 if lo <= v <= hi else 0

def _band_runs(band, gap):
    """(n, y, start) of every row of `band` whose longest fill/ghost run is 4 px or more: the same runs _bar_run
    finds, from a one-byte-per-pixel class map (c = fill/ghost, f = hit flash, . = anything else) built with PIL's own
    image operations and matched with a regular expression, so a frame costs a few milliseconds even with red all
    over it instead of a Python pass over every pixel. A run is coloured pixels with flash gaps of under `gap`."""
    from PIL import ImageChops
    r, g, b = band.convert('RGB').split()
    fill = ImageChops.darker(ImageChops.darker(r.point(_band(220, 255)), g.point(_band(90, 195))), b.point(_band(0, 125)))
    ghost = ImageChops.darker(ImageChops.darker(r.point(_band(165, 215)), g.point(_band(55, 105))), b.point(_band(55, 100)))
    flash = ImageChops.darker(ImageChops.darker(r.point(_band(235, 255)), g.point(_band(235, 255))), b.point(_band(235, 255)))
    colour = ImageChops.lighter(fill, ghost)
    if not colour.getbbox(): return []
    classes = Image.new('L', band.size, ord('.')); classes.paste(ord('f'), mask=flash); classes.paste(ord('c'), mask=colour)
    data, w = classes.tobytes(), band.width
    run = re.compile(rb'c(?:f{0,%d}c)*' % max(0, gap - 1))
    out = []
    for y in range(band.height):
        row = data[y * w:(y + 1) * w]
        if row.count(b'c') < 2: continue                          # a run of 4 or more has both ends coloured
        best = (0, 0)
        for m in run.finditer(row):
            if m.end() - m.start() > best[0]: best = (m.end() - m.start(), m.start())
        if best[0] >= 4: out.append((best[0], y, best[1]))
    return out

def find_bar(im, cropped=False, tries=8):
    """Locate the boss bar. Returns dict(yc, left_bracket, span, track_left, track_right, thickness) or None.
    Red/orange things other than the bar (fire effects, portals) can make longer runs than a drained bar, so the
    best few candidate rows are each checked for the bar's two white brackets; the first that has them wins."""
    W, H = im.size
    y0, y1 = (0, H) if cropped else (0, int(H * 0.22))
    x0, x1 = (0, W) if cropped else (int(W * 0.08), int(W * 0.92))
    # coarse search on the band shrunk to ~960 px wide whatever the resolution (a whole-number factor left 1080p
    # unshrunk and 1440p barely shrunk: the slowest screens to find a bar on)
    f = max(1.0, W / 960); d = max(1, round(f))
    band = im.crop((x0, y0, x1, y1)).resize((max(1, round((x1 - x0) / f)), max(1, round((y1 - y0) / f))), Image.NEAREST)
    runs = _band_runs(band, gap=max(2, round(24 / f)))
    if not runs: LAST_FAIL[0] = 'no red/orange run near the top'; return None
    runs.sort(reverse=True); px = im.load(); tried = []; reasons = []
    for n, by, bs in runs:
        if len(tried) >= tries: break
        if any(abs(by - t) <= 2 for t in tried): continue          # same bar seen on a neighbouring row
        tried.append(by)
        r = _check_candidate(px, W, H, x0 + round(bs * f), y0 + round(by * f), d, cropped)
        if isinstance(r, dict): LAST_FAIL[0] = ''; return r
        reasons.append(r)
    LAST_FAIL[0] = '; '.join(dict.fromkeys(reasons)) or 'no candidate'
    return None

def bar_at(im, spots):
    """The boss bar where it was last seen (it always appears in the same place for one window size): the same full
    checks as find_bar - both white brackets, centred, the xN box or open track - on that one spot. ~1 ms, so the
    meter can look every frame and notice a bar the moment it appears. None if it isn't there."""
    W, H = im.size; d = max(1, round(W / 960)); px = None
    for spot in spots or []:                                    # Interlink and open-world bars can sit at different heights
        if tuple(spot.get('frame', ())) != (W, H) or 'span' not in spot: continue
        px = px or im.load()
        # first the cheap part: both white brackets exactly where they were (a few pixels each, ~0.1 ms)
        yc, t = int(spot['yc']), int(spot['thickness'])
        lb, rb = int(round(spot['left_bracket'])), int(round(spot['left_bracket'] + spot['span']))
        at = lambda x0: next((x for x in range(max(0, x0 - 2), min(W, x0 + 3)) if _is_bracket(px, x, yc, t, W, H)), None)
        if not (0 <= yc < H and rb < W): continue
        lbx = at(lb); rbx = at(rb) if lbx is not None else None
        if lbx is None or rbx is None: continue
        for dx in (2, 8, 20):                                   # just inside the track's left end (fill drains from the right)
            r = _check_candidate(px, W, H, int(spot['track_left']) + dx, int(spot['yc']), d, False, brackets=(lbx, rbx))
            if isinstance(r, dict): return r
    return None

def _check_candidate(px, W, H, xg, yg, d, cropped, brackets=None):
    """Refine one candidate run at full resolution and check it is the boss bar. Returns the bar dict or a reason."""
    col = min(W - 1, xg + 4 * d)
    if not colored(px[col, yg]):
        near = [y for y in range(max(0, yg - d), min(H, yg + d + 1)) if colored(px[col, y])]
        if not near: return 'run vanished at full resolution'
        yg = near[0]
    top = yg
    while top > 0 and colored(px[col, top - 1]): top -= 1
    bot = yg
    while bot < H - 1 and colored(px[col, bot + 1]): bot += 1
    yc = (top + bot) // 2; thick = max(3, bot - top + 1)
    if not cropped and thick > H * 0.03: return f'too thick ({thick}px)'
    if brackets:                                                    # already checked where they were (bar_at)
        lb, rb = brackets; start = xg
    else:
        n, start = _longest_run(px, yc, max(0, xg - 4 * d), W, colored)
        lb = None                                                   # left bracket: thin tall white line
        for x in range(start - 1, max(0, start - 4 * thick) - 1, -1):
            if _is_bracket(px, x, yc, thick, W, H): lb = x; break
        est = SPAN_PER_THICKNESS * thick
        rb = None
        if lb is not None:
            for x in range(min(W - 1, int(lb + 1.25 * est)), int(lb + 0.8 * est), -1):   # rightmost bracket-like line
                if _is_bracket(px, x, yc, thick, W, H): rb = x; break
    if lb is not None and rb is not None: span = rb - lb
    elif not cropped: return 'no left bracket' if lb is None else 'no right bracket'
    else:
        span = est; lb = start - TRACK[0] * span
    world = False
    if not cropped:
        if abs(lb + span / 2 - W / 2) > 0.04 * W: return 'not centred on screen'     # the boss bar is top-centre
        # Boss rush bars end in a dark box with white "xN" digits; bosses out in the world (Alphas) have none and
        # their track runs on to the right bracket - with full HP that part is still red fill.
        x_from, x_to = int(lb + TRACK[1] * span + 0.01 * span), int(lb + 0.99 * span)
        if x_to <= x_from: return 'no room for an xN box'
        rows = [y for y in range(yc - thick // 2, yc + thick // 2 + 1) if 0 <= y < H]
        digits = sum(1 for x in range(x_from, x_to) for y in rows if is_white(px[x, y]))
        dark = sum(1 for x in range(x_from, x_to) if max(px[x, yc][:3]) < 120 or is_white(px[x, yc]))
        red = sum(1 for x in range(x_from, x_to) if colored(px[x, yc]))
        if digits >= 0.03 * (x_to - x_from) * len(rows) and dark >= 0.6 * (x_to - x_from): world = False
        elif dark + red >= 0.8 * (x_to - x_from): world = True                         # dark empty track or HP fill
        else: return 'no xN box or open track at the right end'
        # an Interlink bar whose xN box is gone (x1, late in the fight) looks like a world bar with an empty right end;
        # live HP red there means the track really runs on to the bracket (see as_bossrush_bar)
        right_red = red / (x_to - x_from)
    else: right_red = 0.0
    right = WORLD_TRACK_RIGHT if world else TRACK[1]
    return {'yc': yc, 'left_bracket': lb, 'span': span, 'brackets_found': rb is not None, 'world': world, 'right_red': right_red,
            'track_left': round(lb + TRACK[0] * span), 'track_right': round(lb + right * span), 'thickness': thick,
            'frame': (W, H)}

def as_bossrush_bar(bar):
    """In an Interlink fight a bar found without its xN box (x1, or the box gone near the end) still has the Interlink
    track: the dark right end is the empty box, not track. Only live HP red there says the track runs on."""
    if not bar.get('world') or bar.get('right_red', 0) > 0.2: return bar
    return {**bar, 'world': False, 'track_right': round(bar['left_bracket'] + TRACK[1] * bar['span'])}

def measure_fill(im, bar):
    """Fraction of the track still filled with live HP (white flash and ghost ignored)."""
    px = im.load(); W = im.width; yc = bar['yc']; q = max(1, bar['thickness'] // 4)
    rows = (yc - q, yc, yc + q); left, right = bar['track_left'], min(bar['track_right'], W)
    last = left - 1; flash = ghost = 0
    for x in range(left, right):
        v = [px[x, y] for y in rows]
        if sum(map(is_fill, v)) >= 2: last = x
        elif sum(map(is_flash, v)) >= 2: flash += 1
        elif sum(map(is_ghost, v)) >= 2: ghost += 1
    return max(0.0, min(1.0, (last - left + 1) / (right - left))), flash, ghost

def _median_px(px, x, rows):
    v = sorted((px[x, y][:3] for y in rows), key=sum)
    return v[len(v) // 2]

def _diff(a, b): return abs(a[0] - b[0]) + abs(a[1] - b[1]) + abs(a[2] - b[2])

def measure_fill_any(im, bar, jump=70):
    """Fill fraction without assuming a color: follow the bar from its left end until the color jumps sharply
    (into the hit flash, the ghost, the next segment's color underneath, or the empty track).
    Returns (fraction, fill_color, edge_color). A bar with no jump reads as full."""
    px = im.load(); yc = bar['yc']; q = max(1, bar['thickness'] // 4); rows = (yc - q, yc, yc + q)
    left, right = bar['track_left'], min(bar['track_right'], im.width - 3)
    seen = {}                                            # each column's median once (every one is used three times)
    def med(x):
        v = seen.get(x)
        if v is None: v = seen[x] = _median_px(px, x, rows)
        return v
    fill_color = med(left + 2); edge = right
    for x in range(left + 3, right - 2):
        a = med(x - 2)
        if _diff(a, med(x)) > jump and _diff(a, med(x + 2)) > jump:   # a real edge stays different, not a one-pixel speck
            edge = x - 1; break
    frac = max(0.0, min(1.0, (edge - left + 1) / (right - left)))
    return frac, fill_color, _median_px(px, min(right - 1, edge + 3), rows)

# Break gauge: the thin line under the HP bar. It starts full and drains as break damage lands (white, orange for
# about the last 30%); at zero the boss is broken (the empty part of the HP track turns yellow with "250%") for
# ~10 s, then the line refills and drains again. The track behind the line is see-through, so a line pixel must have
# the bar's dark rim above it and the dark outline below.
BREAK_BELOW = (0.010, 0.019)                      # line rows: fractions of the bracket span below the bar centre
def _gauge_white(p): return min(p[:3]) >= 195 and max(p[:3]) - min(p[:3]) <= 14
def _gauge_orange(p): r, g, b = p[:3]; return r >= 235 and 185 <= g <= 220 and 125 <= b <= 170
def _break_yellow(p): r, g, b = p[:3]; return r >= 235 and g >= 200 and b <= 150
def _dark(p): return max(p[:3]) < 135

def measure_break(im, bar):
    """(fraction of the break gauge left or None when it can't be read, 'white'/'orange'/None, broken)."""
    px = im.load(); W, H = im.size; s = bar['span']; yc = bar['yc']
    left, right = bar['track_left'], min(bar['track_right'], W - 1)
    if right - left < 50: return None, None, False
    yellow = sum(1 for x in range(left, right, 4) if _break_yellow(px[x, yc])) / ((right - left) / 4)
    broken = yellow > 0.05
    gap = max(2, round(0.004 * s))                # rim/outline distance from a line row
    y0, y1 = int(yc + BREAK_BELOW[0] * s), min(H - gap - 1, int(yc + BREAK_BELOW[1] * s) + 1)
    x0 = left + 3
    for y in range(y0, y1):
        for test, kind in ((_gauge_white, 'white'), (_gauge_orange, 'orange')):
            if not (test(px[x0, y]) and test(px[x0 + 3, y])): continue
            if not any(_dark(px[x0, y - d]) for d in range(1, gap + 2)) or not any(_dark(px[x0, y + d]) for d in range(1, gap + 2)):
                continue
            x = x0; miss = 0
            while x < right and (test(px[x, y]) or miss < 2):
                miss = 0 if test(px[x, y]) else miss + 1; x += 1
            return max(0.0, min(1.0, (x - miss - left) / (right - left))), kind, broken
    # no line: empty (broken, or about to refill) - only trust that when the rim is where the line would be
    return (0.0 if broken else None), None, broken

def verify_bar(im, bar):
    """Is the boss bar still where we locked it? Checks the two white brackets only (works for any bar color)."""
    if bar['track_right'] >= im.width or bar['yc'] >= im.height: return False
    px = im.load(); W, H = im.size; t = bar['thickness']
    lb = int(round(bar['left_bracket'])); rb = int(round(bar['left_bracket'] + bar['span']))
    near = lambda x0: any(_is_bracket(px, x, bar['yc'], t, W, H) for x in range(max(0, x0 - 2), min(W, x0 + 3)))
    return near(lb) and near(rb)

def box_at(bar, rel):
    lb, s, yc = bar['left_bracket'], bar['span'], bar['yc']
    return (lb + rel[0] * s, yc + rel[1] * s, lb + rel[2] * s, yc + rel[3] * s)

def timer_box(W, H, wide=False):
    """Boss rush countdown, pinned to the top-left of the 16:9 UI area. Windows OCR needs a tight crop here."""
    u = min(W, H * 16 / 9)                       # UI width (ultrawide screens keep the UI pinned left)
    return (u * 0.02, H * 0.26, u * 0.10, H * 0.33) if wide else (u * 0.03, H * 0.27, u * 0.09, H * 0.32)

def prep_text(img, box, scale, thr=190, size=3, pad=24):
    """White game text -> black on white, padded, at the same text size for any resolution.
    Enlarged before thresholding so small, anti-aliased text keeps its shape."""
    c = img.crop(tuple(int(v) for v in box)).convert('L')
    k = size / scale
    c = c.resize((max(1, int(c.width * k)), max(1, int(c.height * k))), Image.BICUBIC)
    c = ImageOps.invert(c.point(lambda v: 255 if v > thr else 0))
    return ImageOps.expand(c, border=pad, fill=255)

def prep_white(img, box, scale, size=4):
    """Only near-white, unsaturated pixels count as text, so bright sky or sand around a box stays blank."""
    c = img.crop(tuple(int(v) for v in box)).convert('RGB'); k = size / scale
    c = c.resize((max(1, int(c.width * k)), max(1, int(c.height * k))), Image.BICUBIC)
    # bright (every channel over 165) and grey (channels within 50): whole-image operations, not a pixel loop
    # (75-1400 ms a box at 4K that way; the same picture in 6-100 ms)
    r, g, b = c.split()
    low = ImageChops.darker(ImageChops.darker(r, g), b); high = ImageChops.lighter(ImageChops.lighter(r, g), b)
    text = ImageChops.multiply(low.point(lambda v: 255 if v > 165 else 0),
                               ImageChops.subtract(high, low).point(lambda v: 255 if v < 50 else 0))
    return ImageChops.invert(text)

def prep_grey(img, box, scale, size=3, pad=24):
    """Dark grey text on a light card -> black on white; coloured marks next to it (an element icon after a name)
    stay blank, so they don't take the line with them."""
    c = img.crop(tuple(int(v) for v in box)).convert('RGB'); k = size / scale
    c = c.resize((max(1, int(c.width * k)), max(1, int(c.height * k))), Image.BICUBIC)
    # dark (brightest channel under 135) and grey (channels within 45): whole-image operations, not a pixel loop
    # (a 4K Gizmo list took 1.75 s that way)
    r, g, b = c.split()
    high = ImageChops.lighter(ImageChops.lighter(r, g), b); low = ImageChops.darker(ImageChops.darker(r, g), b)
    text = ImageChops.multiply(high.point(lambda v: 255 if v < 135 else 0),
                               ImageChops.subtract(high, low).point(lambda v: 255 if v < 45 else 0))
    return ImageOps.expand(ImageOps.invert(text), border=pad, fill=255)

def prep_bright(img, box, scale, size=2):
    """Any bright text (pink difficulty, orange numbers, yellow titles) on a darker background: brightness,
    stretched, as dark text on white. For coloured text that the white-only preparations drop."""
    c = img.crop(tuple(int(v) for v in box)).convert('L'); k = size / scale
    c = c.resize((max(1, int(c.width * k)), max(1, int(c.height * k))), Image.LANCZOS)
    return ImageOps.expand(ImageOps.invert(ImageOps.autocontrast(c, cutoff=2)), border=20, fill=255)

def prep_labelled(img, box, scale, label, size=2):
    """A lone number (a single '1' is easily skipped by Windows OCR): printed after a label, so it is read as part
    of a line ('Defeats: 1'). The label is drawn here; only the number comes from the game."""
    from PIL import ImageDraw, ImageFont
    c = img.crop(tuple(int(v) for v in box)).convert('L')
    c = c.resize((max(1, int(c.width * size / scale)), max(1, int(c.height * size / scale))), Image.LANCZOS)
    c = ImageOps.invert(ImageOps.autocontrast(c, cutoff=2))
    try: font = ImageFont.truetype('arialbd.ttf', max(12, int(c.height * 0.6)))
    except OSError: font = ImageFont.load_default()
    w = int(font.getlength(label)) + 60
    out = Image.new('L', (c.width + w + 20, c.height + 40), 255)
    ImageDraw.Draw(out).text((20, 20 + c.height * 0.2), label, fill=0, font=font); out.paste(c, (w, 20))
    return out

def prep_labelled_dark(img, box, scale, label, size=3):
    """prep_labelled() for dark text on a light background (the Profile screen's numbers): not inverted."""
    from PIL import ImageDraw, ImageFont
    c = img.crop(tuple(int(v) for v in box)).convert('L')
    c = ImageOps.autocontrast(c.resize((max(1, int(c.width * size / scale)), max(1, int(c.height * size / scale))), Image.LANCZOS), cutoff=2)
    try: font = ImageFont.truetype('arialbd.ttf', max(12, int(c.height * 0.6)))
    except OSError: font = ImageFont.load_default()
    w = int(font.getlength(label)) + 60
    out = Image.new('L', (c.width + w + 20, c.height + 40), 255)
    ImageDraw.Draw(out).text((20, 20 + c.height * 0.2), label, fill=0, font=font); out.paste(c, (w, 20))
    return out

def prep_short(img, box, scale, copies=3):
    """Very short text (e.g. 'x16'): Windows OCR skips it on its own, so repeat it side by side."""
    c = prep_white(img, box, scale); gap = 20
    out = Image.new('L', (c.width * copies + gap * (copies + 1), c.height + 2 * gap), 255)
    for i in range(copies): out.paste(c, (gap + i * (c.width + gap), gap))
    return out

def after_label(text, label):
    """What prep_labelled() read after its drawn label: "Player name: 0" or "Player name: / 0" -> "0"."""
    t = (text or '').strip()
    low, key = t.lower(), label.lower().rstrip(':')
    if key in low: t = t[low.index(key) + len(key):]
    return t.strip(' /:;|.').strip()

def ink_aspect(img, box, thr=200):
    """Width / height of the bright text in a box (a one-character name: a zero is narrow, about 0.6; an O round,
    about 0.9)."""
    b = img.crop(tuple(int(v) for v in box)).convert('L').point(lambda v: 255 if v > thr else 0).getbbox()
    return (b[2] - b[0]) / (b[3] - b[1]) if b else None

# ---- OCR (Windows built-in: ocr-helper.exe when shipped, else a long-running PowerShell helper) ----
OCR_EXE = os.path.join(HERE, 'ocr-helper.exe')   # native helper (app/ocr-helper): grey pixels on stdin, no image files

TEMP_PREFIX = 'bossbar_'

def sweep_temp(hours=1):
    """Temp folders of OCR helpers from earlier runs (a crash or a forced close can't delete its own): gone."""
    import shutil
    cutoff = time.time() - hours * 3600
    try: names = os.listdir(tempfile.gettempdir())
    except OSError: return
    for n in names:
        p = os.path.join(tempfile.gettempdir(), n)
        try:
            if n.startswith(TEMP_PREFIX) and os.path.isdir(p) and os.stat(p).st_mtime < cutoff: shutil.rmtree(p, ignore_errors=True)
        except OSError: pass

class Ocr:
    swept = False
    def __init__(self):
        if not Ocr.swept:                              # in the background: a big Temp folder can take a while
            Ocr.swept = True; threading.Thread(target=sweep_temp, daemon=True, name='temp-sweep').start()
        self.native = os.path.exists(OCR_EXE); self.lock = threading.Lock(); self.n = 0; self.dir = None; self.last = {}
        flags = getattr(subprocess, 'CREATE_NO_WINDOW', 0)
        if self.native:
            self.p = subprocess.Popen([OCR_EXE], stdin=subprocess.PIPE, stdout=subprocess.PIPE, creationflags=flags)
            if self.p.stdout.readline().strip() != b'READY': raise RuntimeError('Windows OCR helper did not start')
            return
        self.p = subprocess.Popen(['powershell.exe', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
                                   os.path.join(HERE, 'ocr_win.ps1'), '-Server'],
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding='utf-8', errors='replace',
                                  creationflags=flags)
        if self.p.stdout.readline().strip() != 'READY': raise RuntimeError('Windows OCR helper did not start')
        self.dir = tempfile.mkdtemp(prefix=TEMP_PREFIX)

    def read(self, images):
        if self.native: return self.read_native(images)
        out = {}
        with self.lock:
            for name, img in images.items():
                self.n += 1   # fresh file name each time: the OCR helper may still hold the previous one open
                path = os.path.join(self.dir, f'{name}_{self.n}.png'); img.save(path)
                self.p.stdin.write(path + '\n'); self.p.stdin.flush()
                line = self.p.stdout.readline().rstrip('\n')
                out[name] = line.split('\t', 1)[1].strip() if '\t' in line else ''
                old, self.last[name] = self.last.get(name), path   # this box's previous file: read, not needed
                if old:
                    try: os.remove(old)
                    except OSError: pass
        return out

    def read_native(self, images):
        """All images in one request: b'OCR1', count, then width, height and 8-bit grey pixels each (u32 LE).
        One line comes back per image, in order."""
        names = list(images); parts = [b'OCR1', struct.pack('<I', len(names))]
        for name in names:
            img = images[name] if images[name].mode == 'L' else images[name].convert('L')
            parts += [struct.pack('<II', *img.size), img.tobytes()]
        with self.lock:
            self.p.stdin.write(b''.join(parts)); self.p.stdin.flush()
            lines = [self.p.stdout.readline() for _ in names]
        return {name: line.decode('utf-8', 'replace').strip() for name, line in zip(names, lines)}

    def close(self):
        try: self.p.stdin.close(); self.p.wait(3)
        except Exception: self.p.kill()
        if self.dir:                                   # the PowerShell helper's image files
            import shutil
            shutil.rmtree(self.dir, ignore_errors=True)

DIGIT_FIX = str.maketrans({'O': '0', 'o': '0', 'Q': '0', 'D': '0', 'I': '1', 'l': '1', 'S': '5', 'B': '8', 'Z': '2', 'G': '6'})

def parse_title(t):
    m = re.search(r'L\s*[vVyYnN]\s*\.?\s*([0-9OoQDIlSBZG]+)', t or '')      # 'Lv', also misread as LV / LY / LN / Ly
    digits = m.group(1).translate(DIGIT_FIX) if m else ''
    # levels are 1-80: a stray mark after the number at small sizes ('Lv. 551 Omega' for Lv 55) keeps its first two
    level = int(digits[:2]) if digits.isdigit() else None
    if level is not None and not 1 <= level <= 80: level = None
    title = (t[m.end():] if m else (t or '')).strip(' |Il')
    return level, strip_sparkling(re.sub(r'\s+', ' ', title).strip())

def strip_sparkling(title):
    """'Sparkling- Young Omega Sherro' -> 'Young Omega Sherro'. Sparkling is a cosmetic variant (one of the Aniilog's
    Sparkling styles): no boss list has a Sparkling title, so pick_boss never matched it. The word is often garbled
    (';parkling-', 'arkdjng-', 'Tfkling-', 'økl}ing-'), so a short head before a dash goes if it is half like
    'sparkling'; without the dash only a close match (no boss name's first word is: 'Tucking' scores 0.5)."""
    for pattern, need in ((r'\s*([^-–—~]{1,13}?)\s*[-–—~]+\s*(\S.*)', 0.5), (r'\s*(\S{6,12})\s+(\S.*)', 0.7)):
        m = re.match(pattern, title or '')
        if m and norm(m.group(1)) and difflib.SequenceMatcher(None, norm(m.group(1)), 'sparkling').ratio() >= need:
            return m.group(2)
    return title

XN_FIX = str.maketrans({'l': '1', 'I': '1', 'i': '1', '|': '1', 'O': '0', 'o': '0', 'D': '0', 'Q': '0', 'S': '5', 's': '5'})

def parse_xn(t):
    """Majority vote over the repeated copies ('x16 / x16 / x16'). This font's 1 is often read as 7 or l/I."""
    votes = []
    for m in re.finditer(r'[xX×]\s*([0-9lIi|OoDQSs]{1,2})(?![0-9])', t or ''):
        s = m.group(1).translate(XN_FIX)
        if not s.isdigit(): continue
        n = int(s)
        if n > 16 and len(s) == 2: n = int(s.replace('7', '1'))      # 'x77' -> 11, 'x70' -> 10
        if 1 <= n <= 16: votes.append(n)
    return max(set(votes), key=votes.count) if votes else None

def parse_timer(t):
    m = re.search(r'(?<!\d)(\d{1,2})\s*[:.]\s*(\d{2})(?!\d)', t or '')
    return int(m.group(1)) * 60 + int(m.group(2)) if m else None

# ---- boss data ----
GAME_BOSSES = [None]      # the boss list read from the player's game data (gamedata.cached_bosses), when there is one

def load_catalog():
    """The shipped boss list, with the game data's own rows (refreshed by the meter after each game patch) taking
    the place of the same (table, puppetId) and adding bosses new since aLogs was built."""
    with open(os.path.join(HERE, 'boss_catalog.csv'), encoding='utf-8') as f: rows = list(csv.DictReader(f))
    game = GAME_BOSSES[0]
    if not game: return rows
    key = lambda r: (r['table'], r['puppetId'])        # one row per boss per table: a patch's changed propId replaces it
    fresh = {key(r): r for r in game}
    return [{**r, **fresh.pop(key(r), {})} for r in rows] + list(fresh.values())

def load_grades():
    with open(os.path.join(HERE, 'boss_rush_grades.csv'), encoding='utf-8') as f:
        return [float(r['cumulative_hp_loss_pct']) / 100 for r in csv.DictReader(f)]   # index = grade 0..16

def norm(s): return re.sub(r'[^a-z]', '', (s or '').lower())

def hp_formula(r):
    try: return json.loads(r['fixed_hp_rate']).get('1')
    except (ValueError, AttributeError): return None

def world_rows(rows):
    """Out in the world, the entries a boss on the bar can be. One name has many entries (Alpha Minespine: 30), and
    taking the first gave Interlink's 33M HP or a quest copy's third of the real HP: the wild one is preferred, the
    standard wild-Alpha HP formula (3101) on a world id (21...), as the game's own Alpha Panpanta Lv 55 matched."""
    rows = [r for r in rows if r['fixed_hp_rate']]
    big = lambda r: r['puppetId'].startswith(('8000', '1108'))          # Interlink-scale copies (tens of millions)
    if rows and all(big(r) for r in rows): return [wild_copy(rows[0])]   # no world entry yet (Alpha Glameep)
    rows = [r for r in rows if not big(r)]
    wild = [r for r in rows if hp_formula(r) == 3101]
    # the entry named as its bar reads ("Young Omega Sherro", 344,742 at Lv 60) over plain ones ("Sherro": 29,731)
    named = [r for r in wild if norm(r['name_en']) == norm(r['bossTitle_en'])]
    return named or [r for r in wild if r['puppetId'].startswith('21')] or wild or rows

def wild_copy(row):
    """A boss with only Interlink entries met in the world (a new Whisperwake Alpha): its species with the standard
    wild-Alpha HP formula, the same as every world Alpha (Minespine, Tubster, Panpanta ...), instead of Interlink's
    tens of millions. An estimate until the game data has the boss's own world entry."""
    return {**row, 'hp_estimated': 'wild', 'prop_species_adj': '0',
            'fixed_hp_rate': '{"1": 3101, "2": 75, "3": 1, "4": 1, "5": 1}'}

def pick_boss(catalog, title, bossrush, boss_id=None):
    # main puppet_data table only; puppet_data1/2/3 are alternate copies that sometimes disagree
    main = [r for r in catalog if r['table'] == 'puppet_data'] or catalog
    if boss_id: rows = [r for r in main if r['puppetId'] == str(boss_id)]
    else:
        t = norm(title)
        rows = [r for r in main if t and t == norm(r['bossTitle_en'])] or [r for r in main if t and t == norm(r['name_en'])]
        if not rows and t:   # OCR slip: closest boss-bar title, if it is clearly close
            titles = {norm(r['bossTitle_en']) for r in main if r['bossTitle_en']}
            close = difflib.get_close_matches(t, titles, n=1, cutoff=0.8)
            if close: rows = [r for r in main if norm(r['bossTitle_en']) == close[0]]
        if bossrush:
            rush = [r for r in rows if r['in_boss_rush'] == 'yes']
            rows = rush or rows
            active = active_cycle_bosses()
            in_active = [r for r in rows if int(r['puppetId']) in active]   # same title, different entry per cycle
            if not in_active and len(rows) > 1:                              # schedule ran out: latest cycle with this boss
                latest = latest_cycle_boss({int(r['puppetId']) for r in rows})
                in_active = [r for r in rows if int(r['puppetId']) == latest]
            if not in_active and active:
                # a clipped title ("Tucking" for "Omega Tucking") matches an open-world boss exactly: in Interlink only
                # this week's bosses can be on the bar, so match the title against those instead
                week = [r for r in main if int(r['puppetId']) in active and r['in_boss_rush'] == 'yes' and r['bossTitle_en']]
                fits = [r for r in week if t and t in norm(r['bossTitle_en'])] or                        [r for r in week if t and difflib.SequenceMatcher(None, t, norm(r['bossTitle_en'])).ratio() >= 0.6]
                if len({r['puppetId'] for r in fits}) == 1: in_active = fits
            # with the week known only its three bosses can be on the bar: nothing else, however close the name
            rows = in_active if active else (rush or rows)
        else: rows = world_rows([r for r in rows if r['in_boss_rush'] != 'yes'] or rows)
    rows = [r for r in rows if r['fixed_hp_rate']]
    return (rows[0] if rows else None), len({(r['species_hp'], r['prop_species_adj'], r['fixed_hp_rate']) for r in rows})

# The boss rush week schedule: bundled, replaced by a fresher copy read from the game data after a patch (live.py)
CYCLES_FILE = [os.path.join(HERE, 'boss_rush_cycles.json')]

def active_cycle_bosses(now=None):
    """Boss ids of the boss rush cycle running now (boss_rush_cycles.json, decoded from the game data)."""
    import time
    now = now or time.time()
    try:
        with open(CYCLES_FILE[0], encoding='utf-8') as f: cycles = json.load(f)
    except OSError: return set()
    return {b for c in cycles if c['start'] <= now <= c['end'] for b in c['bossIds'] + [x for x in c.get('bossIds2') or [] if x]}

def active_cycle_slots(now=None):
    """The running week's bosses by their spot on the boss rush map: {'left': id, 'middle': id, 'right': id}."""
    import time
    now = now or time.time()
    try:
        with open(CYCLES_FILE[0], encoding='utf-8') as f: cycles = json.load(f)
    except OSError: return {}
    for c in cycles:
        if c['start'] <= now <= c['end'] and len(c['bossIds']) == 3:
            return dict(zip(('left', 'middle', 'right'), (str(b) for b in c['bossIds'])))
    return {}

def prep_label(img, u, ux, H):
    """The battle prep screen's big slanted label at the bottom left ('[Middle] Omega Infergon'), levelled for OCR."""
    c = img.crop((int(ux + u * 0.17), int(H * 0.75), int(ux + u * 0.46), int(H * 0.97))).convert('L')
    c = c.rotate(-11, resample=Image.BICUBIC, expand=True, fillcolor=0)
    k = 2000 / u
    c = c.resize((max(1, int(c.width * k)), max(1, int(c.height * k))), Image.BICUBIC)
    return ImageOps.expand(ImageOps.invert(c.point(lambda v: 255 if v > 200 else 0)), border=20, fill=255)

def active_cycle_id(now=None):
    """Id of the boss rush cycle running now, or None when the schedule in the game files doesn't cover today."""
    import time
    now = now or time.time()
    try:
        with open(CYCLES_FILE[0], encoding='utf-8') as f: cycles = json.load(f)
    except OSError: return None
    return next((c['cycle'] for c in cycles if c['start'] <= now <= c['end']), None)

def latest_cycle_boss(ids):
    """Of these boss ids, the one used in the most recent cycle (for when the schedule in the game files runs out)."""
    try:
        with open(CYCLES_FILE[0], encoding='utf-8') as f: cycles = json.load(f)
    except OSError: return None
    for c in sorted(cycles, key=lambda c: c['end'], reverse=True):
        hit = [b for b in c['bossIds'] + [x for x in c.get('bossIds2') or [] if x] if b in ids]
        if hit: return hit[0]
    return None

def cycle_slot(cycle, boss_id):
    """Where a boss fights in a week: ('left' | 'middle' | 'right', the week's split), or (None, split) when it isn't
    one of that week's bosses. A two-boss middle fight counts for both of its bosses."""
    try:
        with open(CYCLES_FILE[0], encoding='utf-8') as f: cycles = json.load(f)
    except OSError: return None, None
    c = next((c for c in cycles if str(c['cycle']) == str(cycle)), None)
    if not c: return None, None
    for slot, a, b in zip(('left', 'middle', 'right'), c['bossIds'], c.get('bossIds2') or [None] * 3):
        if str(boss_id) in (str(a), str(b)): return slot, c.get('split')
    return None, c.get('split')

CALIB = os.path.join(C.DATA, 'calibration.json')

def load_calibration():
    try:
        with open(CALIB, encoding='utf-8') as f: return json.load(f)
    except (OSError, ValueError): return {}

def save_calibration(puppet_id, level, measured_hp, predicted_hp, note):
    cal = load_calibration(); key = f'{puppet_id}@{level}'
    runs = cal.get(key, {}).get('runs', []) + [{'max_hp': round(measured_hp), 'note': note}]
    vals = sorted(r['max_hp'] for r in runs)
    cal[key] = {'predicted': predicted_hp, 'measured': vals[len(vals) // 2], 'runs': runs[-10:]}
    with open(CALIB, 'w', encoding='utf-8') as f: json.dump(cal, f, indent=1)
    return cal[key]['measured']

_BREAK = None
def boss_break_points(row, level):
    """Size of the boss's break gauge in break points (from its stat block's bp_max_v), or None when unknown."""
    global _BREAK
    if _BREAK is None:
        try:
            with open(os.path.join(HERE, 'boss_break.json'), encoding='utf-8') as f: _BREAK = json.load(f)
        except (OSError, ValueError): _BREAK = {}
    b = _BREAK.get(str(row.get('propId')))
    return break_points(b['formula'], b['args'], level) if b else None

def boss_max_hp(row, level, use_calibration=True):
    sp = float(row['species_hp'] or 0) + float(row['prop_species_adj'] or 0)
    predicted = max_hp(sp, json.loads(row['fixed_hp_rate']), level)
    if use_calibration:
        c = load_calibration().get(f"{row['puppetId']}@{level}")
        if c and c.get('measured'): return c['measured']
    return predicted

def damage_dealt(max_hp_value, fill, xn, cum, bossrush=True):
    """Team damage so far. Boss rush: the bar shows only the current star segment (xN = segments left)."""
    if not bossrush or xn is None: return round((1 - fill) * max_hp_value), None
    g = 17 - xn
    if not 1 <= g <= 16: return None, None
    lo, hi = cum[g - 1], cum[g]
    return round((lo + (1 - fill) * (hi - lo)) * max_hp_value), {'grade': g, 'size_hp': round((hi - lo) * max_hp_value),
                                                                  'left_hp': round(fill * (hi - lo) * max_hp_value)}

def read_frame(im, ocr, cropped=False, want=('title', 'xn', 'timer')):
    """Read one frame. Returns dict with bar geometry, fill and the requested text fields."""
    im = im.convert('RGB'); bar = find_bar(im, cropped)
    if not bar: return None
    fill, flash, ghost = measure_fill(im, bar)
    scale = bar['span'] / REF_SPAN; crops = {}
    if 'title' in want: crops['title'] = prep_text(im, box_at(bar, TITLE_BOX), scale)
    if 'xn' in want: crops['xn'] = prep_short(im, box_at(bar, XN_BOX), scale)
    if 'timer' in want and not cropped: crops['timer'] = prep_text(im, timer_box(*im.size), scale)
    text = ocr.read(crops) if crops else {}
    if 'timer' in crops and parse_timer(text.get('timer')) is None:     # second try: wider box, bolder threshold
        text['timer'] = ocr.read({'timer': prep_text(im, timer_box(*im.size, wide=True), scale, thr=170, size=4, pad=40)})['timer']
    out = {'bar': bar, 'fill': fill, 'flash_px': flash, 'ghost_px': ghost, 'ocr': text}
    if 'title' in text: out['level'], out['title'] = parse_title(text['title'])
    if 'xn' in text: out['xn'] = parse_xn(text['xn'])
    if 'timer' in text: out['timer_s'] = parse_timer(text['timer'])
    return out

