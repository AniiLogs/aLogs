"""Each Battle Data value is read in its own box on its own card: a number can't land on another player's card.

    python tests/bd_cells.py

Meter.battle_data_font and cell_value (8 Nov: OCR's text of all the cards listed one card's Damage Taken and Heal last, and
the parser gave them to the next card, whose own were unread; 3 of 8 first reads differed from the last, 12 values wrong):
- the game's font decides a value it can read, whatever OCR's text said;
- a value it can't read comes from OCR of that one box, never from the text of all the cards;
- one that neither can read stays blank, for a later read to fill;
- without the game's font (it can't be read from the install), OCR's text stands as before;
- a box's OCR read: the number at its end, a lone O as 0, and nothing for no number.
Made-up numbers; the readers are stubbed. Exit code 1 on any failure.
"""
import os, sys, tempfile
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_bdcells_')
from PIL import Image
from live import Meter

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

U, H = 3840, 2160
n = 2
def box_of(i, key):
    c = U * (0.5 + (i - (n - 1) / 2) * 0.1943); y = dict(Meter.BD_ROWS)[key]
    return (c + U * 0.015, H * (y - 0.016), c + U * 0.081, H * (y + 0.016))
class Font:
    def __init__(self, values): self.values = values
    def number(self, im, box, **k):
        return next((v for (i, key), v in self.values.items() if box == box_of(i, key)), None)
class Ocr:
    def __init__(self, values): self.values = values; self.asked = []
    def read(self, crops, **k):
        self.asked = sorted(crops); return {k: self.values.get(k, '') for k in crops}
def run(font, ocr_values, read):
    m = Meter.__new__(Meter); m.name_reader = lambda: font; m.ocr = Ocr(ocr_values)
    m.battle_data_font(Image.new('RGB', (U, H), (220, 220, 225)), U, 0, H, read)
    return read, m.ocr.asked

# the case that was sent to the wrong card in the first read: card 2's are unread, its text has card 1's
read = [{'avg_dps': 100, 'break': 200, 'taken': None, 'heal': None}, {'avg_dps': 300, 'break': 400, 'taken': 11, 'heal': 22}]
font = Font({(0, 'avg_dps'): 100, (0, 'break'): 200, (0, 'taken'): 11, (0, 'heal'): 22, (1, 'avg_dps'): 300, (1, 'break'): 400})
got, asked = run(font, {}, read)
check("another card's numbers are not kept", got[1], {'avg_dps': 300, 'break': 400, 'taken': None, 'heal': None})
check('the font fills its own card', got[0], {'avg_dps': 100, 'break': 200, 'taken': 11, 'heal': 22})
check('OCR is asked only for what the font could not read', asked, ['1heal', '1taken'])

read = [{'avg_dps': 1, 'break': 2, 'taken': 3, 'heal': 4}, {'avg_dps': 5, 'break': 6, 'taken': 7, 'heal': 8}]
font = Font({(0, 'avg_dps'): 100, (0, 'break'): 200, (0, 'taken'): 300, (0, 'heal'): 400, (1, 'avg_dps'): 500, (1, 'break'): 600})
got, _ = run(font, {'1taken': 'Damage Taken 700', '1heal': 'O'}, read)
check('a box read by OCR', got[1], {'avg_dps': 500, 'break': 600, 'taken': 700, 'heal': 0})
check('the font over OCR text', got[0], {'avg_dps': 100, 'break': 200, 'taken': 300, 'heal': 400})

read = [{'avg_dps': 1, 'break': 2, 'taken': 3, 'heal': 4}]
got, asked = run(None, {}, read)
check('no game font: the text stands', (got, asked), ([{'avg_dps': 1, 'break': 2, 'taken': 3, 'heal': 4}], []))

for text, want in (('1,234', 1234), ('Heal Amount 56', 56), ('O', 0), ('o', 0), ('0', 0), ('', None), ('Damage Taken', None), ('12 / 34', 34)):
    check(f'cell_value {text!r}', Meter.cell_value(text), want)

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('bd cells: ok')
