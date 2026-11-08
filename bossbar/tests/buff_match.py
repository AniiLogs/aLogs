"""A Special Offering is matched as the game draws it: its whole picture at its size, ring and arrow left out.

    python tests/buff_match.py

Icons.buff (8 Nov: a chosen offering scored 0.2 against the 0.7 it needs, because icons were cached as their trimmed part
stretched square, and a highlighted one has a white ring and arrow around it):
- a narrow glyph with padding in its picture is told from a wide one with the same parts, at the size the game draws it,
  with and without the ring and arrow;
- an empty slot's dots reach neither of the marks read_party_job asks for (0.7 and 0.15 ahead);
- a Season Buff is found at its own size with its "Lv. N" overlapping below.
Made-up icons and screens only (no game files). Exit code 1 on any failure.
"""
import os, sys, tempfile
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_buffs_')
from PIL import Image, ImageDraw
import icons

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

N = icons.BUFF_SIZE
def art(draw):
    a = Image.new('L', (N, N), 0); draw(ImageDraw.Draw(a)); return a
def battery(d):                     # narrow, with padding: as a whole picture it's nothing like a square
    d.rounded_rectangle((18, 12, 46, 54), 4, fill=255); d.ellipse((26, 18, 38, 30), fill=0); d.rectangle((23, 35, 41, 40), fill=0); d.rectangle((23, 44, 41, 49), fill=0)
def block(d):                       # the same parts, wide
    d.rounded_rectangle((8, 18, 56, 46), 4, fill=255); d.ellipse((26, 22, 38, 34), fill=0); d.rectangle((13, 36, 51, 39), fill=0); d.rectangle((13, 41, 51, 44), fill=0)
def glass(d):
    d.polygon([(16, 12), (48, 12), (32, 32)], fill=255); d.polygon([(16, 52), (48, 52), (32, 32)], fill=255)
ic = icons.Icons.__new__(icons.Icons)
ic.buffs = {'1': {'icon': 'battery', 'kind': 'offering', 'name': 'Battery'}, '2': {'icon': 'block', 'kind': 'offering', 'name': 'Block'},
            '3': {'icon': 'glass', 'kind': 'offering', 'name': 'Glass'}, '4': {'icon': 'season', 'kind': 'season', 'name': 'Season'},
            '5': {'icon': 'other', 'kind': 'season', 'name': 'Other'}}
ic.buff_art = {'battery': art(battery), 'block': art(block), 'glass': art(glass), 'season': art(glass), 'other': art(block)}

BOX = (100, 100, 192, 192); W = BOX[2] - BOX[0]; CX, CY = (BOX[0] + BOX[2]) / 2, (BOX[1] + BOX[3]) / 2
def scene(glyph=None, ring=False, dots=False, scale=0.88):
    im = Image.new('RGB', (300, 300), (140, 170, 215)); d = ImageDraw.Draw(im); R = W * 0.52
    d.ellipse((CX - R, CY - R, CX + R, CY + R), fill=(60, 125, 215))
    if ring:
        d.ellipse((CX - R - 5, CY - R - 5, CX + R + 5, CY + R + 5), outline=(245, 250, 255), width=7)
        d.polygon([(CX - 18, CY - R - 14), (CX + 18, CY - R - 14), (CX, CY - R + 10)], fill=(90, 240, 225))
    if dots:
        for dx in (-20, 0, 20): d.ellipse((CX + dx - 6, CY - 6, CX + dx + 6, CY + 6), fill=(235, 240, 250))
    if glyph:
        side = int(W * scale); g = ic.buff_art[glyph].resize((side, side), Image.LANCZOS)
        im.paste((255, 255, 255), (int(CX - side / 2), int(CY - side / 2)), g)
    return im
def accepted(r): return r[1] >= 0.7 and r[2] >= 0.15

for ring in (False, True):
    for glyph in ('battery', 'block', 'glass'):
        r = ic.buff(scene(glyph, ring), BOX, 'offering', text_below=False)
        check(f'{glyph} (ring {ring})', (r[0], accepted(r)), (glyph, True))
    r = ic.buff(scene(None, ring, dots=True), BOX, 'offering', text_below=False)
    check(f'empty slot (ring {ring}) is no offering', accepted(r), False)

im = scene('season', scale=1.03); d = ImageDraw.Draw(im); d.rectangle((110, 165, 180, 188), fill=(235, 240, 250))    # its "Lv. N"
r = ic.buff(im, BOX, 'season')
check('Season Buff under its level', (r[0], r[1] >= 0.7), ('season', True))

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('buff match: ok')
