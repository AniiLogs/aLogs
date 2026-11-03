"""The boss level drawn in red: the game turns the title's 'Lv.65' red (after a red warning triangle) when the
player's Aniimo is under the boss's level. Read as white text only, it was lost and the fight never started
(report 11-03: 'reading boss name... ("Omega Jeburst" Lv None)' all fight).

    python tests/red_level.py

A boss title is drawn in the game's own font (gamefont, as the meter extracts it from the game install) on the bar's
dark blue, at 1440p's size and scaled to 720p and 4K, and goes through the meter's title reads (prep_text, has_red/prep_red, parse_title_pair) and Windows OCR:
- a red level reads as its number, the name as itself (from the white read);
- a white level with a red element icon on the right asks for no second read and reads as before;
- parse_title takes the dot after 'Lv' as Windows OCR returns it after a red level (a bullet), and as a middle dot or
  comma.
Needs the real OCR helper (bossbar/ocr-helper.exe) and the game's fonts in bossbar/gamefont. Exit code 1 on any failure.
"""
import os, shutil, sys, tempfile
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
TMP = tempfile.mkdtemp(prefix='alogs_redlevel_')
os.environ['ALOGS_DATA'] = TMP
from PIL import Image, ImageDraw
import live, reader as R, gamefont

FONT = gamefont.GameFont.load(os.path.join(live.HERE, 'gamefont'), live.PLAYER_LOG)
if not FONT: print("the game's font can't be read (no game install?): nothing checked"); sys.exit(1)

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

for text, want in (('Lv•65 Omega Jeburst', 65), ('Lv·65 Omega Jeburst', 65), ('Lv,65 Omega Jeburst', 65),
                   ('Lv. 65 Omega Jeburst', 65), ('A Lv•65 Omega Jeburst O', 65)):
    check(f'parse_title {text!r}', R.parse_title(text), (want, 'Omega Jeburst' + (' O' if text.endswith('O') else '')))
check('parse_title_pair: white level kept', R.parse_title_pair('Lv. 60 Alpha Glameep', 'Lv. 65 junk'), (60, 'Alpha Glameep'))
check('parse_title_pair: red level, white name', R.parse_title_pair('! Omega Jeburst', 'A Lv•65 Omega Jeburst'),
      (65, '! Omega Jeburst'.strip(' |Il')))

RED, WHITE, BG = (205, 62, 58), (250, 250, 250), (28, 52, 96)
def title(red_level, W=997, H=52):
    """The title box at 1440p (bar span 926): warning triangle + level + name + element icon."""
    im = Image.new('RGBA', (W, H), BG + (255,)); d = ImageDraw.Draw(im)
    if red_level:
        d.polygon([(220, 44), (250, 6), (280, 44)], fill=RED); im.alpha_composite(FONT.render('!', 28), (238, 2))
    lv = FONT.render('Lv. 65', 34, fill=RED if red_level else WHITE); im.alpha_composite(lv, (290, (H - lv.height) // 2))
    nm = FONT.render('Omega Jeburst', 40); im.alpha_composite(nm, (430, (H - nm.height) // 2)); im = im.convert('RGB')
    d = ImageDraw.Draw(im)
    d.ellipse((745, 8, 781, 44), fill=(222, 70, 52))              # a fire icon's red disc, right of the name
    return im

ocr = R.Ocr()
for red in (True, False):
    for size, k in (('720p', 0.5), ('1440p', 1.0), ('4K', 1.5)):
        im = title(red)
        im = im.resize((round(im.width * k), round(im.height * k)), Image.LANCZOS)
        box, scale = (0, 0, im.width, im.height), 926 * k / R.REF_SPAN
        crops = {'title': R.prep_text(im, box, scale)}
        check(f'{"red" if red else "white"} level at {size}: has_red', R.has_red(im, box, scale), red)
        if R.has_red(im, box, scale): crops['title_red'] = R.prep_red(im, box, scale)
        t = ocr.read(crops)
        level, name = R.parse_title_pair(t['title'], t.get('title_red'))
        check(f'{"red" if red else "white"} level at {size}: level', level, 65)
        check(f'{"red" if red else "white"} level at {size}: name ends', (name or '').split()[-2:], ['Omega', 'Jeburst'])
ocr.close(); shutil.rmtree(TMP, ignore_errors=True)

if fails: print('\n'.join(['FAIL ' + f for f in fails])); sys.exit(1)
print('red level: all checks passed')
