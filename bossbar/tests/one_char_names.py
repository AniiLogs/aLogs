"""One-character player names on the result screen, and how long the name cache keeps a read it isn't sure of.

    python tests/one_char_names.py

A result screen's lower half is drawn: in each slot a hexagonal portrait frame whose pale foot reaches down to the name
pill under it, the dark pill, and a made-up name in the game's own font (gamefont, as the meter extracts it from the game
install), at 4K and scaled to 1440p, 1080p, 900p and 720p as a smaller screen shows it. Every name box goes through
Meter.result_names and Ocr.read as result_screen reads them:
- a one-character name reads as itself (both Latin reads keep the pill and the portrait's foot around a lone character
  and read it as nothing or another character; the Korean model's read of the tidied box doesn't), its look-alikes
  (q and 9, O and 0, c and C) told apart by the game's font;
- a longer name still reads as itself, and a slot with no player (no pill, only the portrait) reads as nothing;
- a read of nothing is kept in the name cache while its screen stays up and UNSURE_S after: the same box on a later
  appearance is read again, not answered from the cache for the rest of the session. A read it's sure of stays.
Needs the real OCR helper (bossbar/ocr-helper.exe) and the game's fonts in bossbar/gamefont. The data folder is a
temporary one. Exit code 1 on any failure.
"""
import os, shutil, sys, tempfile, types
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
TMP = tempfile.mkdtemp(prefix='alogs_onechar_')
os.environ['ALOGS_DATA'] = TMP
from PIL import Image, ImageDraw, ImageFilter
import live, reader as R, gamefont, hangulnames as K

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

fonts = os.path.join(live.HERE, 'gamefont')
LATIN = gamefont.GameFont.load(fonts, live.PLAYER_LOG)
KOREAN = gamefont.GameFont.load(fonts, live.PLAYER_LOG, gamefont.KOREAN_FONT) if LATIN else None
if not KOREAN: print("the game's fonts can't be read (no game install?): nothing checked"); sys.exit(1)
R.Ocr.swept = True
R.Ocr.korean = K.KoreanNames(LATIN, KOREAN)

def screen(names, W=3840, H=2160):
    """The result screen's slots with these names ('' for a slot with no player), drawn at 4K and scaled to W x H."""
    s = 1.0
    im = Image.new('RGB', (3840, 2160), (96, 66, 70))
    d = ImageDraw.Draw(im)
    for i, name in enumerate(names):
        cx, py = 3840 * (0.185 + i * 0.1765), 2160 * 0.6347          # the slot's middle, the pill's middle
        top, bot, hw = py - 330 * s, py - 18 * s, 120 * s
        hexa = [(cx - hw, top + 60 * s), (cx, top), (cx + hw, top + 60 * s), (cx + hw, bot - 60 * s), (cx, bot), (cx - hw, bot - 60 * s)]
        d.polygon(hexa, fill=(70, 72, 92), outline=(205, 205, 215), width=6)
        d.ellipse((cx - 80, top + 60, cx + 80, top + 170), fill=(190, 200, 230))
        d.rectangle((cx - 14, top + 170, cx + 14, bot - 4), fill=(215, 215, 225))      # the pale foot over the pill
        if not name: continue
        r = LATIN.render(name, 44); b = r.getchannel('A').getbbox()
        pw, ph = max(80, b[2] - b[0] + 44), 50
        d.rounded_rectangle((cx - pw / 2, py - ph / 2, cx + pw / 2, py + ph / 2), radius=ph / 2, fill=(62, 60, 70),
                            outline=(120, 116, 128), width=3)
        im.paste(r, (round(cx - (b[0] + b[2]) / 2), round(py - (b[1] + b[3]) / 2)), r)
    im = im.filter(ImageFilter.GaussianBlur(0.6))
    return im if W == 3840 else im.resize((W, H), Image.LANCZOS)

def boxes(im):
    W, H = im.size; u = min(W, H * 16 / 9)
    return live.Meter.result_names(im, u, (W - u) / 2, H)

ocr = R.Ocr()
SIZES = {'4K': (3840, 2160), '1440p': (2560, 1440), '1080p': (1920, 1080), '900p': (1600, 900), '720p': (1280, 720)}
SCREENS = [['B', 'e', '4', ''], ['Y', 'Moxie', '', 'a'], ['', 'd', 'Rin_42', '9'],
           # look-alikes the Korean model's pick alone got wrong (q as 9, small letters as capitals): the game's font
           # settles them
           ['q', 'c', 's', 'v'], ['w', 'u', 'n', '']]
for size, (W, H) in SIZES.items():
    for names in SCREENS:
        crops, grey = boxes(screen(names, W, H))
        ocr.name_cache.clear()
        got = ocr.read(crops, names={k: 24 for k in crops}, raws=grey)
        check(f'{size} {names}', [got[f'name{i}'] for i in range(4)], names)
    # z and O are missed at some sizes (read as nothing, kept for UNSURE_S only), but never read as Z or 0
    crops, grey = boxes(screen(['z', 'O', '', ''], W, H))
    ocr.name_cache.clear()
    got = ocr.read(crops, names={k: 24 for k in crops}, raws=grey)
    check(f'{size} z: itself or nothing', got['name0'] in ('z', ''), True)
    check(f'{size} O: itself or nothing', got['name1'] in ('O', ''), True)

# the name cache: a sure read stays, nothing (a slot with no player) is read again once UNSURE_S has passed
crops, grey = boxes(screen(['B', 'Moxie', '', '']))
ocr.name_cache.clear()
ocr.read(crops, names={k: 24 for k in crops}, raws=grey)
names = {k: 24 for k in crops}
check('cache: every box answered from it at once', ocr.cached(crops, names), True)
real, clock = R.time, [R.time.monotonic()]
R.time = types.SimpleNamespace(monotonic=lambda: clock[0])
UNSURE_S = getattr(R, 'UNSURE_S', 30)
try:
    # while the screen stays up, looked at every 2 s (names_first: cached()), the empty slot isn't read again
    for _ in range(3 * UNSURE_S // 2):
        clock[0] += 2
        if not ocr.cached(crops, names): break
    check('cache: the empty slot, the screen up for 3 x UNSURE_S', ocr.cached({'n': crops['name2']}, {'n': 24}), True)
    clock[0] += UNSURE_S + 1                                  # the screen gone that long: a later appearance
    check('cache: later, the one-character name still is', ocr.cached({'n': crops['name0']}, {'n': 24}), True)
    check('cache: later, the longer name still is', ocr.cached({'n': crops['name1']}, {'n': 24}), True)
    check('cache: later, the empty slot is read again', ocr.cached({'n': crops['name2']}, {'n': 24}), False)
    check('cache: later, the read again', ocr.read(crops, names=names, raws=grey), {'name0': 'B', 'name1': 'Moxie', 'name2': '', 'name3': ''})
finally: R.time = real
ocr.close()

shutil.rmtree(TMP, ignore_errors=True)
if fails:
    print(f'{len(fails)} FAILED:'); [print('  ' + f) for f in fails]; sys.exit(1)
print('One-character names: read at 720p-4K, empty slots empty, unsure reads not cached for good')
