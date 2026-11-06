"""The Party screen's team tiles are found by their skill badge and read with the portrait in its whole square.

    python tests/party_tiles.py

- icons.whole puts an atlas-trimmed sprite back where it sat in its whole rect (textureRectOffset, from the bottom
  left): a portrait 200x256 of 256x256 at x 56 isn't centred; an untrimmed one is left as it is.
- Icons.badge finds a drawn skill badge (a dark round with a light rim, any glyph inside) from a guess
  12 px off (within 2 px: the tile's fit refines a few px either way).
- Meter.team_tile finds a tile whose card sits 9 px left and 4 px up of where it's measured, and reads the portrait
  drawn there as the game lays it out (204 units, 74 left of the badge), over a decoy portrait that differs; and the
  same tile faded into a red card by a tile mask (the cache's team fade), read with the fade drawn in.
Made-up pictures only (no game files, no screenshots). Exit code 1 on any failure.
"""
import os, sys, tempfile, types
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_tiles_')
from PIL import Image, ImageChops, ImageDraw, ImageFilter
import icons, live

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

# -- whole(): the trimmed part back in place ------------------------------------------------------------------------
def sprite(img, rect, off):
    return types.SimpleNamespace(image=img, m_Rect=types.SimpleNamespace(width=rect[0], height=rect[1]),
                                 m_RD=types.SimpleNamespace(textureRectOffset=types.SimpleNamespace(x=off[0], y=off[1])))
part = Image.new('RGBA', (200, 256), (255, 0, 0, 255))
w = icons.whole(sprite(part, (256, 256), (56.03, 0)))
check('whole: size', w.size, (256, 256))
check('whole: left of the part is empty', w.getpixel((55, 128))[3], 0)
check('whole: the part starts at 56', w.getpixel((56, 128))[3], 255)
low = Image.new('RGBA', (246, 231), (0, 255, 0, 255))
w = icons.whole(sprite(low, (256, 256), (0, 0)))
check('whole: offset from the bottom', (w.getpixel((10, 24))[3], w.getpixel((10, 25))[3]), (0, 255))
same = Image.new('RGBA', (256, 256), (0, 0, 255, 255))
check('whole: untrimmed as it is', icons.whole(sprite(same, (256, 256), (0, 0))).size, (256, 256))

# -- a made-up Party screen tile ------------------------------------------------------------------------------------
W, H = 3840, 2160
u, ux = W, 0
unit = live.Meter.TEAM_PITCH * u / 240

def portrait(colours):
    """A 256 portrait: a few coloured blocks on transparent, so two portraits differ clearly."""
    img = Image.new('RGBA', (256, 256)); d = ImageDraw.Draw(img)
    for i, c in enumerate(colours):
        d.rectangle((20 + 54 * i, 40 + 30 * (i % 2), 70 + 54 * i, 230), fill=c + (255,))
    return img

def draw_badge(im, x, y):
    d = ImageDraw.Draw(im); r = 44 * unit
    d.ellipse((x - r, y - r, x + r, y + r), fill=(225, 225, 230))
    r = 42 * unit
    d.ellipse((x - r, y - r, x + r, y + r), fill=(61, 61, 80))
    d.polygon([(x - 15, y + 12), (x, y - 20), (x + 15, y + 12)], fill=(255, 255, 255))     # a glyph

cache = tempfile.mkdtemp(prefix='alogs_icons_')
os.makedirs(os.path.join(cache, 'heads'))
mine = portrait([(200, 40, 40), (40, 160, 60), (240, 200, 30), (60, 60, 220)])
decoy = portrait([(60, 60, 220), (240, 200, 30), (40, 160, 60), (200, 40, 40)])
for key, img in (('10001', mine), ('10002', decoy)):
    img.resize((icons.HEAD_SIZE, icons.HEAD_SIZE), Image.LANCZOS).save(os.path.join(cache, 'heads', key + '.png'))
import json
json.dump({'version': icons.CACHE_VERSION, 'build': 'test', 'heads': {'10001': 'Mine', '10002': 'Decoy'}, 'buffs': {},
           'sim': {}, 'sim_gizmos': {}}, open(os.path.join(cache, 'icons.json'), 'w'))
ic = icons.Icons(cache)

card, i = 2, 1
a, b = live.Meter.PARTY_CARDS[card][:2]
t = a + live.Meter.TEAM_PITCH * i
sx, sy, side = live.Meter.TEAM_SQUARE
px, py = ux + u * (t + sx + side / 2) - 9, H * (b + sy) + u * side / 2 - 4      # the card sits 9 px left, 4 up
screen = Image.new('RGB', (W, H), (150, 165, 190))
s = round(204 * unit)
screen.paste(mine.resize((s, s), Image.BILINEAR), (round(px - s / 2), round(py - s / 2)), mine.resize((s, s), Image.BILINEAR))
bx, by = px + 74 * unit, py
draw_badge(screen, bx, by)

found = ic.badge(screen, bx + 12, by - 6, 42 * unit, 14)
check('badge found within 2 px', (abs(found[1] - bx) <= 2, abs(found[2] - by) <= 2), (True, True))
check('badge score', found[0] > 0.6, True)
M = live.Meter.__new__(live.Meter)
r = M.team_tile(ic, screen, t, b, u, ux, H, M.card_shift(ic, screen, a, b, u, ux, H))
check('tile read', r and r[0], 'Mine')
check('tile sure', bool(r and r[1] >= 0.68 and r[2] >= 0.10), True)

# -- the same tile drawn the game's way: the portrait faded into a red card by the tile's mask --------------------------
fade = Image.new('L', (104, 104)); ImageDraw.Draw(fade).ellipse((4, 20, 80, 86), fill=240)     # bright left of centre
fade = fade.filter(ImageFilter.GaussianBlur(10))
cache2 = tempfile.mkdtemp(prefix='alogs_icons_'); os.makedirs(os.path.join(cache2, 'heads'))
for key, img in (('10001', mine), ('10002', decoy)):
    img.resize((icons.HEAD_SIZE, icons.HEAD_SIZE), Image.LANCZOS).save(os.path.join(cache2, 'heads', key + '.png'))
fade.save(os.path.join(cache2, 'team_fade.png'))
json.dump({'version': icons.CACHE_VERSION, 'build': 'test', 'heads': {'10001': 'Mine', '10002': 'Decoy'}, 'buffs': {},
           'sim': {}, 'sim_gizmos': {}, 'team_fade': True}, open(os.path.join(cache2, 'icons.json'), 'w'))
ic2 = icons.Icons(cache2)
check('fade loaded', ic2.fade is not None and ic2.fade.size, (104, 104))
screen2 = Image.new('RGB', (W, H), (190, 60, 60))                                     # a red card showing through
art = mine.resize((s, s), Image.BILINEAR)
big = round(s * 208 / 204); o = (big - s) // 2
shown = ImageChops.multiply(art.getchannel('A'), fade.resize((big, big), Image.BILINEAR).crop((o, o, o + s, o + s)))
screen2.paste(art.convert('RGB'), (round(px - s / 2), round(py - s / 2)), shown)
draw_badge(screen2, bx, by)
r = M.team_tile(ic2, screen2, t, b, u, ux, H, M.card_shift(ic2, screen2, a, b, u, ux, H))
check('faded tile read', r and r[0], 'Mine')
check('faded tile sure', bool(r and r[1] >= 0.68 and r[2] >= 0.10), True)
check('no fade in the cache: as before', ic.fade, None)

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('party tiles: ok')
