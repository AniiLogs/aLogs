"""A Battle Data number goes out only from its own box, by the game's digits, on cards at rest, and read alike twice.

    python tests/bd_cells.py

Meter.battle_data_font, bd_cards_in_place and results.confirmed (8 Nov: OCR's text of all the cards put one card's Damage
Taken and Heal on the next card; 3 of 8 first reads differed from the last, 12 values wrong):
- the game's digits decide, whatever OCR's text said, and OCR is never asked for a number;
- a value they can't read stays blank, for a later read; below 4K it's tried again on the frame enlarged;
- without the game's font, OCR's text stands;
- cards at rest are in place, at 4K and scaled down; moved sideways or down, or with a card missing, they aren't, and a frame
  like that gives no values at all;
- a number read once isn't confirmed, twice is, and a wrong first read loses to the right one read twice.
Made-up numbers and screens; the digit reader is stubbed. Exit code 1 on any failure.
"""
import os, sys, tempfile
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_bdcells_')
from PIL import Image, ImageChops, ImageDraw
import results
from live import Meter

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

W, H = 3840, 2160
def screen(n=2, w=W, h=H):
    """Cards on a dark scene, drawn where the game has them at rest."""
    im = Image.new('RGB', (W, H), (40, 55, 90)); d = ImageDraw.Draw(im)
    for i in range(n):
        c = W * (0.5 + (i - (n - 1) / 2) * 0.1943)
        d.rectangle((c - W * 0.083, H * 0.30, c + W * 0.083, H * 0.7468), fill=(225, 228, 238))
    return im if w == W else im.resize((w, h), Image.LANCZOS)
class Asked(Exception): pass
class Ocr:
    def read(self, *a, **k): raise Asked()
class Font:
    """The digits of each box by its place: {(card, row): value}; `want` the frame width it answers on."""
    def __init__(self, values, n=2, width=W): self.values, self.n, self.width, self.calls = values, n, width, 0
    def number(self, im, box, **k):
        self.calls += 1
        if im.width != self.width: return None
        u = W * self.width / W / 1
        for (i, key), v in self.values.items():
            c = self.width * (0.5 + (i - (self.n - 1) / 2) * 0.1943); x = c + self.width * 0.015
            if abs(box[0] - x) < 3:
                ys = {k2: y for k2, y in Meter.BD_ROWS}
                y_top = (ys[key] - 0.016)
                if abs(box[1] / im.height - y_top) < 0.002 or abs(box[1] - (ys[key] - 0.016 - 0.54) * 2160) < 3: return v
        return None
def run(font, read, im=None):
    m = Meter.__new__(Meter); m.name_reader = lambda: font; m.ocr = Ocr()
    im = im or screen(len(read)); u = min(im.width, im.height * 16 / 9)
    m.battle_data_font(im, u, (im.width - u) / 2, im.height, read)
    return read

read = [{'avg_dps': 100, 'break': 200, 'taken': None, 'heal': None}, {'avg_dps': 300, 'break': 400, 'taken': 11, 'heal': 22}]
font = Font({(0, 'avg_dps'): 100, (0, 'break'): 200, (0, 'taken'): 11, (0, 'heal'): 22, (1, 'avg_dps'): 300, (1, 'break'): 400})
got = run(font, read)
check("another card's numbers go", got[1], {'avg_dps': 300, 'break': 400, 'taken': None, 'heal': None})
check('its own card gets them', got[0], {'avg_dps': 100, 'break': 200, 'taken': 11, 'heal': 22})

read = [{'avg_dps': 1, 'break': 2, 'taken': 3, 'heal': 4}]
got = run(Font({}, n=1), read)
check('nothing readable: all blank, OCR not asked', got, [{'avg_dps': None, 'break': None, 'taken': None, 'heal': None}])

read = [{'avg_dps': 1, 'break': 2, 'taken': 3, 'heal': 4}]
got = run(None, read)
check('no game font: the text stands', got, [{'avg_dps': 1, 'break': 2, 'taken': 3, 'heal': 4}])

# below 4K: a value the frame can't give comes from the frame enlarged to 4K's size
font = Font({(0, 'avg_dps'): 700}, n=1, width=W)
got = run(font, [{}], im=screen(1, 1920, 1080))
check('1080p: read on the enlarged frame', got[0]['avg_dps'], 700)

# -- cards in place --------------------------------------------------------------------------------------------------
place = lambda im, n: Meter.bd_cards_in_place(im, min(im.width, im.height * 16 / 9), (im.width - min(im.width, im.height * 16 / 9)) / 2, im.height, n)
check('at rest', place(screen(4), 4), True)
for size in ((2560, 1440), (1920, 1080), (1280, 720)):
    check(f'at rest, {size[1]}p', place(screen(4, *size), 4), True)
for dx, dy in ((20, 0), (60, 0), (-20, 0), (0, 8), (0, 20), (0, 40), (0, -40)):
    check(f'moved {dx},{dy}', place(ImageChops.offset(screen(4), dx, dy), 4), False)
check('a card short', place(screen(3), 4), False)
check('none', place(screen(4), 0), False)
got = run(Font({(0, 'avg_dps'): 576}, n=1), [{'avg_dps': 57619}], im=ImageChops.offset(screen(1), 60, 0))
check('a sliding frame gives no values', got[0]['avg_dps'], None)

# -- read twice ------------------------------------------------------------------------------------------------------
votes = {}
results.vote(votes, 'a', 576)
check('read once: not confirmed', results.confirmed(votes, 'a'), None)
results.vote(votes, 'a', 57619)
check('two different reads: not confirmed', results.confirmed(votes, 'a'), None)
results.vote(votes, 'a', 57619)
check('read twice: confirmed, over the first', results.confirmed(votes, 'a'), 57619)
results.vote(votes, 'b', None)
check('nothing read: nothing', results.confirmed(votes, 'b'), None)
results.vote(votes, 'z', 0); results.vote(votes, 'z', 0)
check('a 0 read twice is a 0', results.confirmed(votes, 'z'), 0)

# -- the fight's measuring waits for confirmed numbers ------------------------------------------------------------------
import types
m = Meter.__new__(Meter); seen = []
m.time_check = lambda f, dps, run: seen.append(dps)
fight = types.SimpleNamespace(elapsed=10, damage=0, max_hp=0, bossrush=False, started_at_top=False, level=60, log=types.SimpleNamespace(name='x.csv'))
m.run_of = lambda f: {'damage': 1000, 'title': 'Boss', 'bosses': None}
m.awaiting = (fight, 100)
msg, f = m.bd_measure('saved', [], 50)
check('no numbers yet: nothing measured, still waiting', (seen, m.awaiting is not None), ([], True))
msg, f = m.bd_measure('saved', [70, 30], 51)
check('numbers in: measured once, done waiting', (seen, m.awaiting, f is fight), ([100], None, True))
check('its result is noted', 'vs meter 100' in msg, True)

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('bd cells: ok')
