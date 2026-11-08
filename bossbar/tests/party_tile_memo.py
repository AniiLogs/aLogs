"""A Party screen tile that hasn't changed since the last read keeps that read's answer; one that changed is read again.

    python tests/party_tile_memo.py

Meter.team_tile (8 Nov: a read took 14 s, most of it matching tiles that were the same as the last read):
- the same picture at the same place is answered from the last read (the matcher isn't asked);
- a few levels of noise over it still count as the same picture;
- a different portrait there, or the tile at another place, is matched again;
- each tile is remembered on its own (another card's tile isn't taken for it).
Made-up pictures; the matcher is stubbed. Exit code 1 on any failure.
"""
import os, sys, tempfile, random
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_tilememo_')
from PIL import Image, ImageDraw
from live import Meter

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

W, H = 3840, 2160; u, ux = W, 0
calls = []
class Icons:
    fade = None
    def badge(self, *a): return (0, 0, 0)
    def head(self, im, window, square, fade=False):
        calls.append(window); return ('Read', 0.9, 0.5, 'k')

m = Meter.__new__(Meter); ic = Icons()
a, b = Meter.PARTY_CARDS[0][:2]
def screen(colour, noise=0):
    im = Image.new('RGB', (W, H), (40, 40, 60)); d = ImageDraw.Draw(im)
    px, py = m.tile_centre(a, b, u, ux, H)
    d.ellipse((px - 80, py - 80, px + 80, py + 80), fill=colour)
    if noise:
        random.seed(7)
        for _ in range(4000):
            x, y = random.randrange(int(px) - 120, int(px) + 120), random.randrange(int(py) - 120, int(py) + 120)
            r, g, bl = im.getpixel((x, y)); im.putpixel((x, y), (min(255, r + noise), g, bl))
    return im

def read(im, shift=(0, 0), memo=(0, 0)):
    n = len(calls); m.team_tile(ic, im, a, b, u, ux, H, shift, memo=memo); return len(calls) - n

check('first read matches', read(screen((200, 60, 60))), 1)
check('same picture: kept', read(screen((200, 60, 60))), 0)
check('a few levels of noise: kept', read(screen((200, 60, 60), noise=3)), 0)
check('another portrait: read again', read(screen((60, 200, 60))), 1)
check('moved 30 px: read again', read(screen((60, 200, 60)), shift=(30, 0)), 1)
check('another tile, same picture: read on its own', read(screen((60, 200, 60)), shift=(30, 0), memo=(1, 0)), 1)
check('no memo: always read', read(screen((60, 200, 60)), memo=None), 1)

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('party tile memo: ok')
