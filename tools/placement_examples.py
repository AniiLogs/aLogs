"""The DPS window's example pictures in the app (DPS Meter › Overlay › Placement), drawn at the meter's own spots.

    python tools/placement_examples.py <overworld 4K frame> [out folder]

The overworld pictures are a saved 4K fight frame (Captures & logs › Save test pictures) with the DPS window drawn in
the game font where live.py's show_dps puts it, cropped to the part of the screen it sits in. The Interlink pictures
start from the two earlier ones in app/ui/img, which share one frame: each is clean where the other had the window, so
together they give the frame without it. Keep the spot numbers here in step with show_dps."""
import os, sys
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, '..', 'bossbar'))
import gamefont

FONT = gamefont.GameFont.load(os.path.join(os.environ['LOCALAPPDATA'], 'aLogs', 'meter', 'gamefont'), None)
LINES = [('12,480 DPS', (255, 211, 106), 0.0235, True), ('Last 5 s 14.2K DPS · 41 s', (255, 255, 255), 0.015, False),
         ('Team damage 512K / 1.84M', (255, 255, 255), 0.015, False)]

def window(H, scale=1.0):
    """The DPS window as live.py's compose draws it (no backdrop), for a HUD height H."""
    H = H * scale; pad = max(3, round(H * 0.004)); parts = []
    for text, colour, size, big in LINES:
        px = max(11, round(H * size)); parts.append((FONT.render(text, px, colour, italic=big, bold=big), px))
    width = max(im.width for im, _ in parts); height = sum(im.height + round(px * 0.04) for im, px in parts)
    out = Image.new('RGBA', (width + 2 * pad, height + 2 * pad), (0, 0, 0, 0)); y = pad
    for im, px in parts:
        out.alpha_composite(im, (pad, y)); y += im.height + round(px * 0.04)
    return out

def overworld(frame, out):
    base = Image.open(frame).convert('RGBA'); W, H = base.size; u = min(W, H * 16 / 9); ux = (W - u) / 2
    # right of the minimap: centred in the gap to the boss bar (0.195-0.32 of the width), smaller (show_dps MAP_*)
    win = window(H, 0.85); left, right = ux + u * 0.195, ux + u * 0.32
    x = max(left + H * 0.01, (left + right) / 2 - H * 0.21 * 0.85 / 2)
    im = base.copy(); im.alpha_composite(win, (round(x), round(H * 0.069)))
    im.crop((0, 0, W // 2, H // 2)).convert('RGB').resize((480, 270), Image.LANCZOS).save(os.path.join(out, 'world-map.jpg'), quality=90)
    # top right, under the game's buttons
    im = base.copy(); im.alpha_composite(window(H), (round(ux + u * 0.705), round(H * 0.10)))
    im.crop((W // 2, 0, W, H // 2)).convert('RGB').resize((480, 270), Image.LANCZOS).save(os.path.join(out, 'world-right.jpg'), quality=90)

def interlink(old, out):
    corner, panel = (Image.open(os.path.join(old, n)).convert('RGBA') for n in ('rush-corner.jpg', 'rush-panel.jpg'))
    clean = corner.copy()
    clean.paste(panel.crop((50, 0, 300, 75)), (50, 0))     # the corner picture's window, right of Esc, from the other
    # the crop is the top-left quarter of a 16:9 frame: the HUD is 960 x 540 at this size; drawn 4x and shrunk
    k = 4; H = 540
    for name, (fx, fy) in (('rush-corner.jpg', (0.068, 0.022)), ('rush-panel.jpg', (0.025, 0.398))):
        big = clean.resize((clean.width * k, clean.height * k), Image.LANCZOS)
        big.alpha_composite(window(H * k), (round(960 * k * fx), round(H * k * fy)))
        big.resize(clean.size, Image.LANCZOS).convert('RGB').save(os.path.join(out, name), quality=90)

if __name__ == '__main__':
    out = sys.argv[2] if len(sys.argv) > 2 else os.path.join(HERE, '..', 'app', 'ui', 'img')
    overworld(sys.argv[1], out)
    interlink(os.path.join(HERE, '..', 'app', 'ui', 'img'), out)
    print('saved in', out)
