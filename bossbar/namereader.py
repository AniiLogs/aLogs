"""Short in-game names ("0", "Ab") read by matching each character against the game's own font.

Windows OCR drops a word of one or two characters, so the game menu's name under your UID (see Meter.game_menu) was
never read for players like "0". Here the name is read the way the game draws it: the game's UI font
(UI_Font_Europe, from gamefont.py), upright, about 50 px tall at 4K in the menu and 61 px on the Profile screen. The
name is cut out of the background by its colour - the menu's white text (bright and grey; the scene behind it is
tinted or dark), or the Profile card's dark text, cut out by its own colour since players pick the card's
picture - split into characters at the gaps, and each character is compared with every letter and digit drawn in that font at the size
the screen's resolution implies; size counts as much as shape, which is what tells o from O and 0 from O. A name is
only returned when every character matches clearly.

Pillow only (the packaged meter has no numpy). Checked on a real 4K menu (and it scaled to 1440p, 1080p and 900p):
the name read exactly at each size; the UID's digits, every zero included, at 4K and 1440p.
"""
import string
from PIL import Image, ImageChops, ImageDraw, ImageStat

CHARS = string.ascii_uppercase + string.ascii_lowercase + string.digits
NAME_PX_4K = 50        # the menu name's font size on a 3840-wide 16:9 screen (scales with it)
PROFILE_PX_4K = 61     # the Profile screen's name
STATS_PX_4K = 43.5     # the Profile screen's Collection Progress numbers
EDGE = 120             # the distance-field level the game's text edge sits at (fitted to a real frame)
MIN_SCORE = 0.6        # a character must overlap its best match at least this much...
MIN_MARGIN = 0.05      # ...and beat the next best by this much
TWINS = set('Il')      # a capital I and a small l: the same bar, 2 px apart in height at 4K - never vouched for


def text_mask(img, box, bright=190, grey=50):
    """The box's white, colourless pixels (the game's white text) as a 1-bit image cut to their extent, or None."""
    c = img.crop(tuple(int(v) for v in box)).convert('RGB')
    r, g, b = c.split()
    lo = ImageChops.darker(ImageChops.darker(r, g), b)
    hi = ImageChops.lighter(ImageChops.lighter(r, g), b)
    white = lo.point(lambda v: 255 if v > bright else 0)
    flat = ImageChops.subtract(hi, lo).point(lambda v: 255 if v < grey else 0)
    m = ImageChops.multiply(white, flat)
    bbox = m.getbbox()
    return m.crop(bbox).convert('1') if bbox else None


def ink_colour(img, box, far=80):
    """(text colour, background colour) in a box of dark or light text on a plain-ish card: the background is its
    commonest colour, the text that of the pixels farthest from it (the text is the box's strongest contrast, more
    than a coloured mark after it). (None, bg) if nothing stands out."""
    c = img.crop(tuple(int(v) for v in box)).convert('RGB')
    q = c.quantize(16, method=Image.Quantize.MEDIANCUT); pal = q.getpalette()
    i = max(q.getcolors())[1]; bg = tuple(pal[3 * i:3 * i + 3])
    d = [ImageChops.difference(band, Image.new('L', c.size, v)) for band, v in zip(c.split(), bg)]
    dist = ImageChops.lighter(ImageChops.lighter(d[0], d[1]), d[2])
    top = dist.getextrema()[1]
    if top < far: return None, bg
    core = dist.point(lambda v: 255 if v >= top * 0.85 else 0)
    return tuple(int(v) for v in ImageStat.Stat(c, core).median), bg


def drop_icon(mask):
    """The mask without the small round icon some name cards show in front of the name (a ring with a bar): a first
    shape about as wide as tall, with a wide gap after it. A name's own first letter sits close to the next."""
    parts, x, spans = [], 0, []
    for p in characters_at(mask): spans.append(p)
    if len(spans) < 2: return mask
    (x0, x1, h), (nx0, _, _) = spans[0], spans[1]
    w = x1 - x0
    if 0.8 <= w / max(1, h) <= 1.25 and nx0 - x1 >= 0.35 * mask.height:
        rest = mask.crop((nx0, 0, mask.width, mask.height)); bbox = rest.getbbox()
        return rest.crop(bbox) if bbox else mask
    return mask


def gender_mark(img, box, far=60):
    """'female', 'male' or None: the Profile name card's mark after the name (♀ or ♂). The game tints one white sprite
    per gender, so it's told by shape, not colour: the mark is the card's last coloured shape (neither the card nor
    the name's ink) wide enough to be one, and its ring's hole sits up-right of the mark's middle on ♀ (the cross hangs
    down-left) and down-left on ♂ (the arrow points up-right)."""
    ink, bg = ink_colour(img, box)
    if not ink: return None
    c = img.crop(tuple(int(v) for v in box)).convert('RGB')
    def away(colour):
        d = [ImageChops.difference(band, Image.new('L', c.size, v)) for band, v in zip(c.split(), colour)]
        return ImageChops.lighter(ImageChops.lighter(d[0], d[1]), d[2]).point(lambda v: 255 if v > far else 0)
    mark = ImageChops.multiply(away(bg), away(ink))
    runs, start, last = [], None, None                   # runs of inked columns
    for x in range(mark.width):
        if mark.crop((x, 0, x + 1, mark.height)).getbbox():
            if start is None or x - last > 2: runs.append([x, x]); start = x
            runs[-1][1] = last = x
    runs = [r for r in runs if r[1] - r[0] + 1 >= max(4, mark.height * 0.2)]
    if not runs: return None
    part = mark.crop((runs[-1][0], 0, runs[-1][1] + 1, mark.height))
    bbox = part.getbbox()
    part = part.crop(bbox)
    w, h = part.size
    if h < 8 or not 0.5 <= w / h <= 1.6: return None
    # the hole: background the outside can't reach (flood the outside from a 1 px border)
    room = Image.new('L', (w + 2, h + 2), 0)
    room.paste(part, (1, 1))
    ImageDraw.floodfill(room, (0, 0), 128)
    hole = room.crop((1, 1, w + 1, h + 1)).point(lambda v: 255 if v == 0 else 0)
    px, n, sx, sy = hole.load(), 0, 0, 0
    for y in range(h):
        for x in range(w):
            if px[x, y]: n += 1; sx += x; sy += y
    if n < w * h * 0.02: return None
    dx, dy = sx / n - w / 2, sy / n - h / 2
    if dx > 0 and dy < 0: return 'female'
    if dx < 0 and dy > 0: return 'male'
    return None


def characters_at(mask):
    """[(left, right, height)] of each shape between empty columns."""
    out, start = [], None
    for x in range(mask.width + 1):
        ink = x < mask.width and mask.crop((x, 0, x + 1, mask.height)).getbbox() is not None
        if ink and start is None: start = x
        elif not ink and start is not None:
            b = mask.crop((start, 0, x, mask.height)).getbbox(); out.append((start, x, b[3] - b[1])); start = None
    return out


def ink_mask(img, box, ink, bg):
    """The box's pixels nearer the text colour than the background (each channel within half their gap), as text_mask."""
    c = img.crop(tuple(int(v) for v in box)).convert('RGB')
    tol = max(abs(a - b) for a, b in zip(ink, bg)) // 2
    d = [ImageChops.difference(band, Image.new('L', c.size, v)) for band, v in zip(c.split(), ink)]
    m = ImageChops.lighter(ImageChops.lighter(d[0], d[1]), d[2]).point(lambda v: 255 if v < tol else 0)
    bbox = m.getbbox()
    return m.crop(bbox).convert('1') if bbox else None


def characters(mask):
    """The mask split at its empty columns: [1-bit image of each character, cut to its extent], left to right.
    Specks are left out."""
    out, start = [], None
    for x in range(mask.width + 1):
        ink = x < mask.width and mask.crop((x, 0, x + 1, mask.height)).getbbox() is not None
        if ink and start is None: start = x
        elif not ink and start is not None:
            part = mask.crop((start, 0, x, mask.height)); bbox = part.getbbox()
            small = mask.height * 0.3                  # a speck (the card's edge, a stray pixel), not a character
            if bbox and (bbox[2] - bbox[0] >= small or bbox[3] - bbox[1] >= small): out.append(part.crop(bbox))
            start = None
    return out


def _count(img):
    return img.convert('L').histogram()[255]


def overlap(a, b, slack=2):
    """Intersection over union of two 1-bit shapes, at the best small shift of one against the other."""
    w, h = max(a.width, b.width) + 2 * slack, max(a.height, b.height) + 2 * slack
    A = Image.new('1', (w, h), 0); A.paste(a, (slack, slack))
    best = 0.0
    for dy in range(-slack, slack + 1):
        for dx in range(-slack, slack + 1):
            B = Image.new('1', (w, h), 0); B.paste(b, (slack + dx, slack + dy))
            union = _count(ImageChops.logical_or(A, B))
            if union: best = max(best, _count(ImageChops.logical_and(A, B)) / union)
    return best


class NameReader:
    def __init__(self, font):
        self.font = font                     # gamefont.GameFont (its distance field)
        self.cache = {}

    def glyph(self, ch, px):
        key = (ch, px)
        if key not in self.cache:
            field, _ = self.font._field(ch, px)
            m = field.point(lambda v: 255 if v > EDGE else 0)
            bbox = m.getbbox()
            self.cache[key] = m.crop(bbox).convert('1') if bbox else None
        return self.cache[key]

    def best(self, part, px, chars=CHARS):
        """[(score, character)] best first, for one character's shape."""
        scores = []
        for ch in chars:
            g = self.glyph(ch, px)
            if g is None or abs(g.height - part.height) > max(3, part.height // 3): continue     # far off in size
            if abs(g.width - part.width) > max(3, part.width // 3): continue                   # (or two letters touching)
            scores.append((overlap(part, g), ch))
        return sorted(scores, reverse=True)

    def read(self, img, box, u, max_chars=3, px_4k=NAME_PX_4K, mask=None, chars=CHARS):
        """The name in `box` (u = the width of the screen's 16:9 area), or None unless every character is clear.
        The menu's white text is found by itself; other text comes as its `mask` (ink_mask)."""
        if mask is None: mask = text_mask(img, box)
        if mask is None: return None
        parts = characters(mask)
        if not parts or len(parts) > max_chars: return None
        guess = None
        base = px_4k * u / 3840
        for px in sorted({round(base) - 1, round(base), round(base) + 1}):
            if px < 8: continue
            picks = []
            for part in parts:
                s = self.best(part, px, chars)
                if not s: break
                picks.append((s[0][0], s[0][0] - (s[1][0] if len(s) > 1 else 0), s[0][1]))
            if len(picks) != len(parts): continue
            worst = min(p[0] for p in picks)
            if guess is None or worst > guess[0]: guess = (worst, min(p[1] for p in picks), ''.join(p[2] for p in picks))
        if not guess or guess[0] < MIN_SCORE or guess[1] < MIN_MARGIN or TWINS & set(guess[2]): return None
        return guess[2]
