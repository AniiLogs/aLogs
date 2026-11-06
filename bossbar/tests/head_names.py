"""Head portraits are named and kept the way the Party screen needs them.

    python tests/head_names.py

- icons.head_name names a portrait by the tables' own iconName first; a form's portrait (7 digits) that no row names
  goes by its species (the first 5 digits); Male and Female portraits each keep their own key but the Aniimo's name;
  an unnamed portrait (an Aniimo not out yet) stays out.
- The patch sets' picture bundles (xptNN_mres_expng_*) are among those read for heads.
- An Aniimo whose two gendered heads differ is read from either one (each is its own picture in the cache, so one no
  longer overwrites the other).
Made-up names and pictures only (no game files, no screenshots). Exit code 1 on any failure.
"""
import json, os, sys, tempfile
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_heads_')
from PIL import Image, ImageDraw
import icons

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

# icon -> [(pet id, name)], as extract builds it from pet_prototype_data / pet_data
by_icon = {'10111': [('1011100', 'Pebblet'), ('101110001', 'Pebblet (Boss)')],
           '10112': [('1011200', 'Rockling')],
           '1011101': [('1011101', 'Pebblet Bloom')],
           '10222': [('100031', 'Raincloud'), ('1022200', 'Sparkfox'), ('102220000', 'Alpha Sparkfox')],
           '10333': [('1033300', 'Twinpaw')]}
check('own row', icons.head_name('10111', by_icon), 'Pebblet')
check('a form a row names', icons.head_name('1011101', by_icon), 'Pebblet Bloom')
check('a form no row names: its species', icons.head_name('1011102', by_icon), 'Pebblet')
check('the species own row over other rows sharing the icon', icons.head_name('1022203', by_icon), 'Sparkfox')
check('gendered: the Aniimo', (icons.head_name('10333_female', by_icon), icons.head_name('10333_male', by_icon)),
      ('Twinpaw', 'Twinpaw'))
check('gendered form: its species', icons.head_name('1033301_male', by_icon), 'Twinpaw')
check('unnamed species: left out', icons.head_name('10999', by_icon), None)
check('unnamed form: left out', icons.head_name('1099901', by_icon), None)
check('a 5-digit icon is not cut', icons.head_name('10113', by_icon), None)

# the patch sets' exported pictures are read too (a patch's new heads live there), not their other bundles
for name, want in (('xpt24_mres_expng_2_0123456789abcdef0123456789abcdef.uab', True),
                   ('xpt31_mexp_expng_0_0123456789abcdef0123456789abcdef.uab', True),
                   ('xpt24_mres_exprefab_0_0123456789abcdef0123456789abcdef.uab', False),
                   ('ar_xgui_image_pet_he_8a565150_png_0_0123456789abcdef.uab', False)):
    check(f'patch pictures: {name[:22]}', bool(icons.PATCH_PNG.match(name)), want)

# -- both gendered heads in the cache, each read -------------------------------------------------------------------------
def portrait(colours):
    img = Image.new('RGBA', (256, 256)); d = ImageDraw.Draw(img)
    for i, c in enumerate(colours):
        d.rectangle((20 + 54 * i, 40 + 30 * (i % 2), 70 + 54 * i, 230), fill=c + (255,))
    return img
R, G, Y, B = (200, 40, 40), (40, 160, 60), (240, 200, 30), (60, 60, 220)
pics = {'10333_female': portrait([R, G, Y, B]), '10333_male': portrait([B, Y, R, G]), '10112': portrait([G, B, G, R])}
names = {k: icons.head_name(k, by_icon) for k in pics}
cache = tempfile.mkdtemp(prefix='alogs_icons_'); os.makedirs(os.path.join(cache, 'heads'))
for k, img in pics.items():
    img.resize((icons.HEAD_SIZE, icons.HEAD_SIZE), Image.LANCZOS).save(os.path.join(cache, 'heads', k + '.png'))
json.dump({'version': icons.CACHE_VERSION, 'build': 'test', 'heads': names, 'buffs': {}, 'sim': {}, 'sim_gizmos': {}},
          open(os.path.join(cache, 'icons.json'), 'w'))
ic = icons.Icons(cache)
for k in ('10333_female', '10333_male'):
    screen = Image.new('RGB', (400, 400), (150, 165, 190))
    art = pics[k].resize((200, 200), Image.BILINEAR); screen.paste(art, (100, 100), art)
    r = ic.head(screen, (110, 110, 290, 290), (100, 100, 200))
    check(f'{k} read', r and (r[0], r[3]), ('Twinpaw', k))
    check(f'{k} sure', bool(r and r[1] >= 0.68 and r[2] >= 0.10), True)

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('head names: ok')
