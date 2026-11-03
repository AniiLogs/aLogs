"""Korean player names (Hangul), which Windows' English OCR can't see: it read "ㄴㄱ" as "O", or as nothing.

The OCR helper also runs a Korean text model on name boxes (ocr-helper.exe, OCR2 requests) and sends each letter's
spot and likeliest characters. That model knows Hangul and Latin, but some Korean letters are drawn almost like Latin
ones in the game's font: its ㄱ is a 7 without the slant, its ㄴ a wide L. So each letter is settled against the game's
own fonts - the English UI_Font_Europe and the Korean UI_Font_Korea, both read from the player's game install
(gamefont.py) - by shape (the letter's ink against each candidate drawn at the same size) and by its width to height,
which tells a wide ㄴ from a narrow L. A name only comes out in Hangul when the game's Korean letters fit it clearly;
otherwise the English OCR's read stands, so Latin names read exactly as before.

Pillow only (the packaged meter has no numpy).
"""
import difflib, json, re
from PIL import Image, ImageChops, ImageFilter

INK = 80              # darker than this is the name's ink (the boxes come as dark text on white)
MIN_P = 1e-4          # a candidate the model gives less than this isn't worth drawing
GLYPH_PX = 64         # candidates are drawn this size, then scaled to the letter's ink
MIN_FIT = 0.55        # every Hangul letter must fit its game-font shape at least this well
NOT_A_LETTER = 0.3    # ink no candidate fits even this well is an icon or a mark, not a letter
LATIN_SAME = 0.8      # the English OCR's read this like the model's Latin read: a Latin name, left alone
SPECK = 5             # ink thinner than this many px is a speck, not part of a letter (the boxes are enlarged 2-4x)
SPACE = 0.6           # a gap this many letter heights wide is a space
# Korean letters the game draws almost like a Latin one: when the model sees the Latin one, the Korean one is drawn
# too, and the game's own fonts decide
TWINS = {'7': 'ㄱ', 'L': 'ㄴ', '2': 'ㄹ', 'Z': 'ㄹ', '0': 'ㅇ', 'O': 'ㅇ', 'o': 'ㅇ', 'T': 'ㅜ', 'E': 'ㅌ', 'C': 'ㄷ',
         'l': 'ㅣ', 'I': 'ㅣ', '1': 'ㅣ', '-': 'ㅡ', 'H': 'ㅐ', 'A': 'ㅅ', 'V': 'ㅅ', 'D': 'ㅁ', 'S': 'ㅈ'}


def is_hangul(c):
    return 'ᄀ' <= c <= 'ᇿ' or '㄰' <= c <= '㆏' or '가' <= c <= '힣'


def has_hangul(text):
    return any(is_hangul(c) for c in text or '')


def split_reply(line):
    """The helper's answer to a name box: (the English OCR's text, the Korean model's letters or None, the Latin
    model's text)."""
    fields = line.split('\t')
    text, extra, latin = (fields + ['', ''])[:3]
    try: letters = json.loads(extra) if extra else None
    except ValueError: letters = None
    return text.strip(), letters, latin.strip()


def hangul_first(letters):
    """True when the Korean model's own best letters hold Hangul: KoreanNames.read then decides without the Latin
    read it's given (that only counts against a Latin-looking best read)."""
    return bool(letters) and has_hangul(''.join(picks[0][0] for _, picks in letters if picks))


def clean_latin(text):
    """The Latin model's read of a name box without what follows the name: a voice or ready icon read as a circled
    number or a mark ('Soul_ShadowXD①', 'Neg③.')."""
    text = re.sub(r'[①-⓿❶-➓。｡.,·•…]+$', '', (text or '').strip()).strip()
    return re.sub(r'\s*_\s*', '_', text)      # the game leaves a gap after '_' that reads as a space at 4K


def tidy(img, pad=0):
    """A prepared name box (dark text on white, `pad` px of white added around it) as the Korean model and the font
    check see it: dark regions touching the box's edge (the pill or card around the name, the scene) cleared, the
    rest cut out with a margin and centred on white, so the name fills the model's height. None when no ink is
    left."""
    from PIL import ImageDraw
    grey = img.convert('L')
    if img.width <= 2 * pad or img.height <= 2 * pad: return None
    mask = grey.crop((pad, pad, img.width - pad, img.height - pad)).point(lambda v: 255 if v < INK else 0)
    ring = Image.new('L', (mask.width + 2, mask.height + 2), 255)        # a dark ring joins everything at the edge
    ring.paste(mask, (1, 1))
    ImageDraw.floodfill(ring, (0, 0), 128)
    ink = ring.crop((1, 1, mask.width + 1, mask.height + 1)).point(lambda v: 255 if v == 255 else 0)
    # specks thinner than a stroke (stars, glitter in the scene) go: shrunk away, then the letters grown back
    ink = ink.filter(ImageFilter.MinFilter(SPECK)).filter(ImageFilter.MaxFilter(SPECK))
    # the line of ink nearest the box's middle, where the game puts the name (the foot of a portrait above it goes);
    # bands closer than a third of their height are one line (a Hangul syllable's parts stack with small gaps)
    rows = [ink.crop((0, y, ink.width, y + 1)).getbbox() is not None for y in range(ink.height)]
    bands, start = [], None
    for y, on in enumerate(rows + [False]):
        if on and start is None: start = y
        elif not on and start is not None: bands.append([start, y]); start = None
    merged = []
    for b in bands:
        if merged and b[0] - merged[-1][1] < max(b[1] - b[0], merged[-1][1] - merged[-1][0]) / 3: merged[-1][1] = b[1]
        else: merged.append(b)
    if not merged: return None
    mid = ink.height / 2
    top, bottom = min(merged, key=lambda b: abs((b[0] + b[1]) / 2 - mid))
    line = Image.new('L', ink.size, 0); line.paste(255, (0, top, ink.width, bottom))
    ink = ImageChops.multiply(ink, line)
    box = ink.getbbox()
    if not box: return None
    pad = max(4, round(0.35 * (box[3] - box[1])))
    out = Image.new('L', (box[2] - box[0] + 2 * pad, box[3] - box[1] + 2 * pad), 255)
    out.paste(0, (pad, pad), ink.crop(box))
    return out


def _runs(mask):
    """[(left, right, top, bottom)] of each run of inked columns."""
    out, start = [], None
    for x in range(mask.width + 1):
        ink = x < mask.width and mask.crop((x, 0, x + 1, mask.height)).getbbox() is not None
        if ink and start is None: start = x
        elif not ink and start is not None:
            b = mask.crop((start, 0, x, mask.height)).getbbox()
            out.append((start, x, b[1], b[3])); start = None
    return out


def _iou(a, b, slack=1):
    """Intersection over union of two same-size 1-bit shapes, at the best small shift."""
    w, h = a.width + 2 * slack, a.height + 2 * slack
    A = Image.new('1', (w, h), 0); A.paste(a, (slack, slack))
    best = 0.0
    for dy in range(-slack, slack + 1):
        for dx in range(-slack, slack + 1):
            B = Image.new('1', (w, h), 0); B.paste(b, (slack + dx, slack + dy))
            union = ImageChops.logical_or(A, B).convert('L').histogram()[255]
            if union: best = max(best, ImageChops.logical_and(A, B).convert('L').histogram()[255] / union)
    return best


class KoreanNames:
    def __init__(self, latin_font, korean_font):
        self.fonts = (latin_font, korean_font)       # gamefont.GameFont: UI_Font_Europe, UI_Font_Korea
        self.cache = {}
        self.last = []                               # the last read's letters and fits, for the log

    def glyph(self, ch):
        """The character's shape in the game's font (1-bit, cut to its ink), or None when that font hasn't got it."""
        if ch not in self.cache:
            font = self.fonts[1] if is_hangul(ch) else self.fonts[0]
            shape = None
            if font and len(ch) == 1 and ord(ch) in font.glyphs:
                field, _ = font._field(ch, GLYPH_PX)
                m = field.point(lambda v: 255 if v > 120 else 0); bbox = m.getbbox()
                shape = m.crop(bbox).convert('1') if bbox else None
            self.cache[ch] = shape
        return self.cache[ch]

    def fit(self, part, ch):
        """How well a letter's ink is this character: its shape scaled onto the ink, times how alike their width to
        height is (0..1)."""
        g = self.glyph(ch)
        if g is None: return 0.0
        a, b = part.width / part.height, g.width / g.height
        aspect = min(a, b) / max(a, b)
        return _iou(part, g.resize(part.size, Image.NEAREST)) * aspect

    def read(self, img, text, letters):
        """The name in Hangul when the game's Korean letters fit it, else None (the English OCR's `text` stands).
        `img` is the box as tidy() made it, the picture the model read `letters` from."""
        self.last = []
        if not letters or self.fonts[1] is None: return None
        cands = [[(c, p) for c, p in picks if p >= MIN_P] or picks[:1] for _, picks in letters]
        for picks in cands:
            twin = TWINS.get(picks[0][0])
            if twin and twin not in (c for c, _ in picks): picks.append((twin, 0.0))
        if not any(is_hangul(c) for picks in cands for c, _ in picks): return None
        top = ''.join(picks[0][0] for picks in cands)
        plain = lambda s: re.sub(r'[\W_]', '', s or '').casefold()
        if not has_hangul(top) and plain(text) and difflib.SequenceMatcher(None, plain(text), plain(top)).ratio() >= LATIN_SAME:
            return None                               # the English OCR read this Latin name already
        mask = img.convert('L').point(lambda v: 255 if v < INK else 0)
        runs = _runs(mask)
        if not runs: return None
        # each piece of ink goes to the nearest letter, when it's near enough (an icon the model skipped, or a speck
        # of the pill, belongs to none); a letter's reach is a bit over half the way to its neighbour
        xs = [x for x, _ in letters]
        reach = [0.6 * min([abs(xs[j] - x) for j in (i - 1, i + 1) if 0 <= j < len(xs)] or [img.height])
                 for i, x in enumerate(xs)]
        owner = {}
        for r in runs:
            c = (r[0] + r[1]) / 2
            i = min(range(len(xs)), key=lambda j: abs(xs[j] - c))
            if abs(xs[i] - c) <= reach[i]: owner.setdefault(i, []).append(r)
        chosen = []                                   # (character, its fit, its ink box), left to right
        for i, picks in enumerate(cands):
            mine = owner.get(i)
            if not mine: continue                     # no ink of its own: the scene, or an icon cut away above
            box = (min(r[0] for r in mine), min(r[2] for r in mine), max(r[1] for r in mine), max(r[3] for r in mine))
            part = mask.crop(box).convert('1')
            fits = [(self.fit(part, c), c, p) for c, p in picks]
            fit, c, _ = max(fits, key=lambda f: f[0] + 0.15 * f[2])
            if fit >= NOT_A_LETTER: chosen.append((c, fit, box))     # (nothing fits a voice icon or a ready mark)
        self.last = [(c, round(fit, 2)) for c, fit, _ in chosen]
        if not chosen: return None
        # words: a space where the ink stops for over half a letter's height (the badge, the name and an icon after
        # it are separate words; the game's Korean letters sit closer than that)
        heights = sorted(b[3] - b[1] for _, _, b in chosen)
        wide = SPACE * heights[len(heights) // 2]
        words = [[chosen[0]]]
        for prev, cur in zip(chosen, chosen[1:]):
            if cur[2][0] - prev[2][2] > wide: words.append([])
            words[-1].append(cur)
        # a Korean word: two Hangul letters or more, or one whole syllable, every Hangul letter fitting the game's font
        # clearly (one stray letter like ㅏ is a misread icon or Latin letter, never a name)
        def korean(word):
            hangul = [(c, fit) for c, fit, _ in word if is_hangul(c)]
            return bool(hangul) and all(fit >= MIN_FIT for _, fit in hangul) and \
                (len(hangul) >= 2 or '가' <= hangul[0][0] <= '힣')
        text = lambda word: ''.join(c for c, _, _ in word)
        if not any(korean(w) for w in words): return None
        # the Korean words, and a Party pill's badge in front ('3P'): no specks or icons read as letters around them
        keep = [text(w) for w in words if korean(w) or re.fullmatch(r'[1-4][Pp]', text(w))]
        return ' '.join(keep)
