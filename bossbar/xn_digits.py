"""Reads the boss bar's segment counter (x16 ... x1) by matching digit shapes instead of OCR.

Windows OCR reads two-digit counters well but misses single digits ('x5'), so the counter's fixed font is
matched against digit templates (xn_templates.json) built from labeled frames of a real fight.
"""
import json, os
from PIL import Image

import settings as C
HERE = C.RES
TPL_FILE = os.path.join(HERE, 'xn_templates.json')
GW, GH = 12, 18                      # normalized glyph size
SMALL_H = READ_H = 30                # counter crops shorter than this (720p is 16, 1080p 25) are enlarged to it first

def white_mask(img):
    """1 where the pixel is near-white text (not bright sky or sand), else 0. Built with PIL's own operations."""
    from PIL import ImageChops
    r, g, b = img.convert('RGB').split(); w, h = img.size
    lo = ImageChops.darker(ImageChops.darker(r, g), b); hi = ImageChops.lighter(ImageChops.lighter(r, g), b)
    m = ImageChops.darker(lo.point(lambda v: 1 if v > 165 else 0), ImageChops.subtract(hi, lo).point(lambda v: 1 if v < 60 else 0))
    data = m.tobytes()
    return [list(data[y * w:(y + 1) * w]) for y in range(h)]

def glyphs(mask):
    """Split the mask into glyphs by empty columns; returns list of (x0, x1, y0, y1)."""
    h = len(mask); w = len(mask[0]) if h else 0
    cols = [sum(mask[y][x] for y in range(h)) for x in range(w)]
    out = []; x = 0
    while x < w:
        if cols[x]:
            s = x
            while x < w and cols[x]: x += 1
            ys = [y for y in range(h) if any(mask[y][c] for c in range(s, x))]
            out.append((s, x, ys[0], ys[-1] + 1))
        x += 1
    # drop specks (anti-aliasing dust) - keep glyphs at least 35% of the tallest one
    tall = max((g[3] - g[2] for g in out), default=0)
    return [g for g in out if g[3] - g[2] >= 0.35 * tall and g[1] - g[0] >= 2]

def normalize(mask, g):
    x0, x1, y0, y1 = g
    im = Image.new('L', (x1 - x0, y1 - y0))
    im.putdata([255 * mask[y][x] for y in range(y0, y1) for x in range(x0, x1)])
    im = im.resize((GW, GH), Image.BILINEAR)
    return [v / 255 for v in im.getdata()]

def split_counter(img):
    """Glyph vectors of the digits after the 'x' (the 'x' is the first glyph and is shorter than digits)."""
    m = white_mask(img); gs = glyphs(m)
    if len(gs) < 2: return []
    tall = max(g[3] - g[2] for g in gs)
    digits = [g for g in gs[1:] if g[3] - g[2] >= 0.8 * tall]      # digits are full height, the 'x' is not
    return [normalize(m, g) for g in digits][:2]

def load_templates():
    try:
        with open(TPL_FILE, encoding='utf-8') as f: return json.load(f)
    except (OSError, ValueError): return {}

def match(vec, tpl):
    best, second, bd = None, None, 1e9; sd = 1e9
    for d, t in tpl.items():
        dist = sum(abs(a - b) for a, b in zip(vec, t)) / len(vec)
        if dist < bd: second, sd = best, bd; best, bd = d, dist
        elif dist < sd: second, sd = d, dist
    return best, bd, sd

def read_counter(img, tpl=None, max_dist=0.22):
    """Returns the counter value (1..16) or None when unsure."""
    tpl = tpl or load_templates()
    if not tpl: return None
    if img.height < SMALL_H:          # small counters: the 'x' and a '1' touch and read as one shape (x16 -> 6)
        img = img.resize((max(1, round(img.width * READ_H / img.height)), READ_H), Image.BICUBIC)
    ds = split_counter(img)
    if not ds: return None
    s = ''
    for v in ds:
        d, bd, sd = match(v, tpl)
        if d is None or bd > max_dist or sd - bd < 0.03: return None
        s += d
    n = int(s)
    return n if 1 <= n <= 16 else None
