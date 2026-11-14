"""The game's own pictures behind the Party screen's icons: each Aniimo's head portrait (the team row under a player's
card shows the three benched Aniimo only as pictures) and the Interlink buffs (Season Buffs and the Special Offering show
only as icons, with "Lv. N" under a Season Buff).

Read once from the player's own install (never shipped with aLogs), like gamefont.py: names from pmdata.bin
(pet_prototype_data / pet_data: name + iconName -> sprite UI_PetHead_<iconName>; bossrush_buff_data: buffName, buffIcon,
buffType, Bufflevel), pictures from the UnityPy bundles in StreamingAssets (and the game's downloaded patch bundles,
which hold the newest heads). Cached in the data folder per game build; about 20 s the first time.

Matching is plain Pillow (numpy isn't in the meter build): a masked normalized correlation per colour channel, which
doesn't mind the frosted card or the fade over a picture, only its shape and shading.
"""
import glob, json, os, re, time
from PIL import Image, ImageChops, ImageDraw, ImageFont, ImageStat

CACHE_VERSION = 8          # 2: Holo-Battle Sim's series, Core gizmo and Alpha Transformation pictures; 3: gizmo names;
                           # 4: portraits back in their whole square (the atlas trims them); 5: the team tiles' fade;
                           # 6: heads from the patch sets, form portraits no row names (by species), Male and Female
                           # heads each kept; 7: buff icons at their true shape, not their trimmed part stretched square;
                           # 8: the lead portrait's fade
HEAD_SIZE = 96              # cached head portraits (px square)
BUFF_SIZE = 64
SIM_SIZE = 96               # cached Holo-Battle Sim pictures (longest side)
BUFF_PREFIXES = ('mres_xgui_texture_expng_', 'mres_xgui_panel_expng_', 'mres_xgui_common_expng_', 'ar_xgui_icon_skilli_',
                 'ar_xgui_icon_item_i_')
HEAD_PREFIXES = ('ar_xgui_image_pet_he_', 'ar_xgui_image_petman_')
# ...and the game's patch sets of exported pictures (xpt24_mres_expng_2_*.uab): the heads added in a patch live there
# (Leafy, Sparkelf, Helion, Grizbo... none of them in the head bundles above)
PATCH_PNG = re.compile(r'xpt\d+_m(res|exp)_expng_')
# The Party screen's team tiles fade each portrait into the card with this soft mask (UI_Node_TeamRoom_CenterItem >
# PetMinorN > Mask: 208 units square over the 204 portrait, centred on it): bright left of centre, gone at the badge
TEAM_FADE, FADE_PREFIXES = 'ui_img_grabeggs_centeritem_mask2', ('mres_xgui_rpanel_expng_',)
# ...and the card's lead portrait with its own (UI_Node_TeamRoom_CenterItem > PetPrimary > Mask: 256 units square over
# the 256 portrait): bright over the top left, fading out towards the name and the card's bottom (14 Nov)
LEAD_FADE = 'ui_img_grabeggs_centeritem_mask1'
FADES = {'team': (TEAM_FADE, 208 / 204), 'lead': (LEAD_FADE, 1.0)}     # the mask's side over the portrait's
KIND = {'General': 'season', 'Special': 'offering'}


def _bundle_dir(game):
    return os.path.join(game, 'Aniimo_Data', 'StreamingAssets', 'cvs', 'res', 'uab', 'win', 'DefaultPackage')

def _build(game):
    try:
        with open(os.path.join(_bundle_dir(game), 'PackageManifest_DefaultPackage.version'), encoding='utf-8') as f: return f.read().strip()
    except OSError: return None

def _game(player_log=None):
    import gamedata
    pm = gamedata.find_game_data(player_log)
    return (pm, pm[:pm.lower().rindex('aniimo_data')].rstrip('\\/')) if pm else (None, None)

def whole(sprite):
    """A sprite as the game draws it: the atlas keeps only its drawn part (a portrait 200x256 of its 256x256), and
    textureRectOffset says where that part sits in the whole (from the bottom left); an Image stretches the whole."""
    img = sprite.image.convert('RGBA')
    try:
        w, h = round(sprite.m_Rect.width), round(sprite.m_Rect.height)
        off = sprite.m_RD.textureRectOffset
        if (w, h) == img.size or w < img.width or h < img.height: return img
        out = Image.new('RGBA', (w, h))
        out.paste(img, (round(off.x), h - round(off.y) - img.height))
        return out
    except (AttributeError, TypeError, ValueError):
        return img


def head_name(key, by_icon):
    """The Aniimo a head portrait shows, from its sprite name after UI_PetHead_ (lower case): '10161', '1038101',
    '10161_female'. by_icon: icon -> [(pet id, name)] from the tables' iconName. Each gendered portrait is its own
    picture (one Aniimo's Male and Female heads differ), named as the Aniimo. A form's portrait (7 digits: species +
    form) that no row names is its species' (the first 5 digits): the Party screen draws it for that form's
    Aniimo (a Blazen form, Eko's and Eklue's forms...). None when nothing names it (an Aniimo not out yet)."""
    icon = key.split('_')[0]
    for k in ((icon, icon[:5]) if len(icon) == 7 and icon.isdigit() else (icon,)):
        own = [n for p, n in by_icon.get(k, []) if p in (k, k + '00')]
        names = [n for _, n in by_icon.get(k, [])]
        if own or names: return own[0] if own else max(set(names), key=names.count)
    return None


def extract(cache_dir, player_log=None):
    """Writes <cache_dir>/heads/*.png, buffs/*.png and icons.json from the game install. True on success."""
    import UnityPy, gamedata
    pm_path, game = _game(player_log)
    if not game: return False
    pm, en = gamedata.PmData(pm_path), gamedata.english_text(pm_path)
    buff_rows = {k: r for k, r in (pm.table('bossrush_buff_data') or {}).items() if r.get('buffType') in KIND}
    icon_of = lambda r: (r.get('buffIcon') or '').lstrip('$').rsplit('.', 1)[0]
    wanted = {icon_of(r).lower() for r in buff_rows.values()}
    buff_icons = set(wanted)
    # Holo-Battle Sim (Victory screen): per series id its name, the series row's icon, its Core gizmo, and the Alpha
    # Transformation it unlocks (random_buff_series_name, rogue_transform_data)
    sim = {}
    for sid, r in (pm.table('random_buff_series_name') or {}).items():
        sim[str(sid)] = {'name': en(r.get('buffSeriesName')), 'core': en(r.get('coreBuffName')),
                         'series_icon': icon_of({'buffIcon': r.get('buffSeriesIcon')}).lower(),
                         'core_icon': icon_of({'buffIcon': r.get('coreBuffIcon')}).lower()}
    for sid, r in (pm.table('rogue_transform_data') or {}).items():
        if str(sid) in sim:
            sim[str(sid)].update(transform=en(r.get('transformName')), transform_icon=icon_of({'buffIcon': r.get('transformIcon')}).lower())
    wanted |= {v for s in sim.values() for k, v in s.items() if k.endswith('_icon') and v}
    # ...and every Holo-gizmo by name (the Gizmo screen lists them): series id and rarity (10 Normal, 5 Uncommon, 2 Core)
    rarity = {10: 'Normal', 5: 'Uncommon', 2: 'Core'}
    sim_gizmos = {}
    for r in (pm.table('random_buff_data') or {}).values():
        name = en(r.get('buffName')) if isinstance(r, dict) else None
        if name and r.get('buffSeries') is not None:
            sim_gizmos.setdefault(name, [str(r['buffSeries']), rarity.get(r.get('buffRarity'), 'Special')])
    shipped = sorted(f for f in glob.glob(os.path.join(_bundle_dir(game), '*.uab'))
                     if os.path.basename(f).startswith(BUFF_PREFIXES + HEAD_PREFIXES + FADE_PREFIXES)
                     or PATCH_PNG.match(os.path.basename(f)))
    patches = glob.glob(os.path.join(game, 'Aniimo_Data', 'cvs', 'res', 'uab', 'win', 'DefaultPackage', 'CacheBundleFiles', '*', '*', 'cdata.uab'))
    art = {}                                                   # lower name -> (image, file time); the newest file wins
    for f in shipped + patches:
        try: env = UnityPy.load(f); stamp = os.path.getmtime(f)
        except Exception: continue
        for o in env.objects:
            if o.type.name != 'Sprite': continue
            name = o.peek_name() or ''
            low = name.lower()
            fade = low in (TEAM_FADE, LEAD_FADE)
            if (low in wanted or fade or low.startswith('ui_pethead_')) and (low not in art or art[low][1] < stamp):
                try: art[low] = ((whole(o.read()) if low.startswith('ui_pethead_') or fade or low in buff_icons else o.read().image.convert('RGBA')), stamp)
                except Exception: pass
    for sub in ('heads', 'buffs'): os.makedirs(os.path.join(cache_dir, sub), exist_ok=True)
    # heads: named by the game's own tables only (a portrait the tables don't name is left out)
    forms = {}
    for table in ('pet_prototype_data', 'pet_data'):
        for pid, r in (pm.table(table) or {}).items():
            if isinstance(r, dict) and r.get('iconName') and en(r.get('name')):
                forms.setdefault(str(pid), (str(r['iconName']).split('_')[0], en(r.get('name'))))
    by_icon = {}
    for pid, (icon, name) in sorted(forms.items(), key=lambda kv: int(kv[0]) if kv[0].isdigit() else 0):
        by_icon.setdefault(icon, []).append((pid, name))
    heads = {}
    for low, (img, _) in art.items():
        if not low.startswith('ui_pethead_'): continue
        key = low[len('ui_pethead_'):]
        name = head_name(key, by_icon)
        if not name: continue
        sq = Image.new('RGBA', (max(img.size),) * 2)               # the portrait square (a few are trimmed)
        sq.paste(img, ((sq.width - img.width) // 2, sq.height - img.height))
        sq.resize((HEAD_SIZE, HEAD_SIZE), Image.LANCZOS).save(os.path.join(cache_dir, 'heads', key + '.png'))
        heads[key] = name
    buffs = {}
    for k, r in buff_rows.items():
        low = icon_of(r).lower()
        if low not in art: continue
        art[low][0].resize((BUFF_SIZE, BUFF_SIZE), Image.LANCZOS).save(os.path.join(cache_dir, 'buffs', low + '.png'))
        buffs[str(k)] = {'name': en(r.get('buffName')), 'kind': KIND[r['buffType']], 'icon': low,
                         **({'level': int(r['Bufflevel'])} if r.get('Bufflevel') else {})}
    for kind, (low, _) in FADES.items():
        if low in art: art[low][0].getchannel('A').save(os.path.join(cache_dir, kind + '_fade.png'))
    os.makedirs(os.path.join(cache_dir, 'sim'), exist_ok=True)
    sim_out = {}
    for sid, s in sim.items():
        entry = {k: s[k] for k in ('name', 'core', 'transform') if s.get(k)}
        for part in ('series', 'core', 'transform'):
            low = s.get(part + '_icon')
            if low in art:
                img = art[low][0].copy()
                if part == 'transform': img = img.crop((0, 0, img.width, min(img.width, img.height)))   # the head end of
                img.thumbnail((SIM_SIZE, SIM_SIZE), Image.LANCZOS)                     # a tall portrait, as the round frame shows it
                img.save(os.path.join(cache_dir, 'sim', f'{part}_{sid}.png')); entry[part + '_art'] = True
        sim_out[sid] = entry
    with open(os.path.join(cache_dir, 'icons.json'), 'w', encoding='utf-8') as f:
        json.dump({'version': CACHE_VERSION, 'build': _build(game), 'heads': heads, 'buffs': buffs, 'sim': sim_out,
                   'team_fade': TEAM_FADE in art, 'lead_fade': LEAD_FADE in art,
                   'sim_gizmos': sim_gizmos}, f, ensure_ascii=False)
    return bool(heads and buffs)


import operator
_LEVELS = range(256)
_SQUARES = [j * j for j in _LEVELS]


_ABOVE_64 = [255 if v > 64 else 0 for v in range(256)]     # point() tables: a list, not a call per level
_ABOVE_200 = [255 if v > 200 else 0 for v in range(256)]


class Stat:
    """ImageStat.Stat's count, mean and var, the same numbers (whole-number sums, exact in a float), summed with map
    instead of its per-level Python loop: the Party screen's portrait match takes ~1,200 of these per tile, and
    ImageStat's loops were most of a read's 14 s (8 Nov)."""
    __slots__ = ('count', 'mean', 'var')

    def __init__(self, img, mask=None):
        h = img.histogram(mask)
        self.count, self.mean, self.var = [], [], []
        for b in range(0, len(h), 256):
            band = h[b:b + 256]
            n = sum(band)
            s, s2 = float(sum(map(operator.mul, _LEVELS, band))), float(sum(map(operator.mul, _SQUARES, band)))
            self.count.append(n)
            self.mean.append(s / n if n else 0)
            self.var.append((s2 - (s ** 2.0) / n) / n if n else 0)


def ncc(a, b, mask):
    """Masked normalized correlation of two same-size RGB images, averaged over the channels (-1..1)."""
    sa, sb = Stat(a, mask), Stat(b, mask)
    if not sa.count[0]: return -1
    cross = Stat(ImageChops.multiply(a, b), mask).mean
    out = 0
    for k in range(3):
        cov = cross[k] * 255 - sa.mean[k] * sb.mean[k]
        out += cov / ((sa.var[k] * sb.var[k]) ** 0.5 + 1e-6)
    return out / 3


def ncc_pooled(a, b, mask):
    """Masked normalized correlation of two same-size RGB images over all three channels together (-1..1): unlike
    ncc, a colour that's off counts (a red portrait against a blue one), not only each channel's own pattern."""
    sa, sb = Stat(a, mask), Stat(b, mask)
    if not sa.count[0]: return -1
    cross = Stat(ImageChops.multiply(a, b), mask).mean
    ma, mb = sum(sa.mean) / 3, sum(sb.mean) / 3
    cov = sum(cross[k] * 255 for k in range(3)) / 3 - ma * mb
    va = sum(sa.var[k] + sa.mean[k] ** 2 for k in range(3)) / 3 - ma * ma
    vb = sum(sb.var[k] + sb.mean[k] ** 2 for k in range(3)) / 3 - mb * mb
    return cov / ((va * vb) ** 0.5 + 1e-6)


class Icons:
    """The cached pictures, and the Party screen readers that use them."""
    def __init__(self, cache_dir):
        with open(os.path.join(cache_dir, 'icons.json'), encoding='utf-8') as f: meta = json.load(f)
        if meta.get('version') != CACHE_VERSION: raise ValueError('old icon cache')
        self.build = meta.get('build')
        self.heads = {k: (n, Image.open(os.path.join(cache_dir, 'heads', k + '.png')).convert('RGBA')) for k, n in meta['heads'].items()}
        self.buffs = meta['buffs']
        self.buff_art = {}
        for b in self.buffs.values():
            if b['icon'] not in self.buff_art:
                self.buff_art[b['icon']] = Image.open(os.path.join(cache_dir, 'buffs', b['icon'] + '.png')).convert('RGBA').getchannel('A')
        self.sim = meta.get('sim', {})
        self.sim_gizmos = meta.get('sim_gizmos', {})     # Holo-gizmo name -> [series id, rarity]
        self.sim_art = {}             # (part, series id) -> RGBA picture
        for sid, s in self.sim.items():
            for part in ('series', 'core', 'transform'):
                if s.get(part + '_art'):
                    self.sim_art[(part, sid)] = Image.open(os.path.join(cache_dir, 'sim', f'{part}_{sid}.png')).convert('RGBA')
        # the team tiles' and the lead portrait's fades (square 'L' pictures), when the install has them: head(fade=True)
        # draws the team one in, head(fade='lead') the lead one
        self.fades = {k: Image.open(os.path.join(cache_dir, k + '_fade.png')).convert('L') for k in FADES if meta.get(k + '_fade')}
        self.fade = self.fades.get('team')
        self._sized = {}
        self._faded = {}
        self._cuts = {}
        self._digits = None

    @classmethod
    def load(cls, cache_dir, player_log=None):
        """The cached icons, read from the game again when it was updated since; None when the game can't be read."""
        try:
            icons = cls(cache_dir)
            _, game = _game(player_log)
            if not game or _build(game) in (None, icons.build): return icons
        except (OSError, ValueError, KeyError): pass
        try:
            if extract(cache_dir, player_log): return cls(cache_dir)
            cls.error = 'the game install was not found'
        except Exception as e:
            import traceback
            cls.error = ''.join(traceback.format_exception_only(type(e), e)).strip()
            cls.trace = traceback.format_exc()
        return None

    error = None
    trace = None

    # -- heads ------------------------------------------------------------------------------------------------------
    def _head(self, key, side):
        k = (key, side)
        if k not in self._sized: self._sized[k] = self.heads[key][1].resize((side, side), Image.BILINEAR)
        return self._sized[k]

    def _fade(self, side, kind='team'):
        """A fade over a portrait square of `side` px: the team tiles' (the mask is 208/204 of it, on the same centre) or
        the lead portrait's (the same size as it)."""
        if (side, kind) not in self._faded:
            big = round(side * FADES[kind][1]); o = (big - side) // 2
            self._faded[(side, kind)] = self.fades[kind].resize((big, big), Image.BILINEAR).crop((o, o, o + side, o + side))
        return self._faded[(side, kind)]

    _CUTS = 6000          # cut portraits kept (~5 KB each): the same few dozen offsets recur on every tile and read

    def _cut(self, key, side, box, fade):
        """A portrait at `side` px cut to `box`, as head's score compares it: (RGB, what shows, mask), or None when too
        little of it shows. Kept: they depend on nothing but these four."""
        k = (key, side, box, fade)
        if k in self._cuts: return self._cuts[k]
        part = self._head(key, side).crop(box)
        if fade:
            shown = ImageChops.multiply(part.getchannel('A'), self._fade(side, fade).crop(box))
            mask = shown.point(_ABOVE_64)
            least = 0.15
        else:
            shown, mask, least = None, part.getchannel('A').point(_ABOVE_200), 0.3
        # the mask's mean, as ImageStat has it (0 or 255 a pixel)
        cut = None if 255 * mask.histogram()[255] / (mask.width * mask.height) < least * 255 else (part.convert('RGB'), shown, mask)
        if len(self._cuts) >= self._CUTS: self._cuts.clear()
        self._cuts[k] = cut
        return cut

    def head(self, im, window, square, n=40, fade=False):
        """The Aniimo whose portrait shows through `window` (a screen box), the portrait's whole square being about
        `square` (x, y, side in screen px; it's cut by the window). (name, score, margin over the next name, icon), or None.
        A first pass with the square as placed picks a dozen candidates, a second fits each a few px and % around.
        `fade`: drawn as the Party screen's team tiles draw it (when the cache has the fade): faded by the tile's mask
        into the card (the tile's own mean colour stands in for the card art showing through), and compared where the
        portrait shows at a quarter or more. 'lead': the same with the card's lead portrait's mask. Without it, where
        the portrait is solid."""
        X0, Y0, X1, Y1 = window
        h = max(4, round(n * (Y1 - Y0) / (X1 - X0))); k = n / (X1 - X0)
        crop = im.crop(tuple(int(v) for v in window)).convert('RGB').resize((n, h), Image.BILINEAR)
        fade = 'team' if fade is True else fade
        if fade not in self.fades: fade = False
        if fade: card = Image.new('RGB', (n, h), tuple(int(v) for v in ImageStat.Stat(crop).mean))
        side0 = square[2] * k; ox0 = (square[0] - X0) * k; oy0 = (square[1] - Y0) * k
        def score(key, scale, dx, dy):
            side = int(side0 * scale)
            ox = int(round(ox0 + (side0 - side) / 2)) + dx; oy = int(round(oy0 + (side0 - side) / 2)) + dy
            if ox > 0 or oy > 0 or ox + side < n or oy + side < h: return -1
            box = (-ox, -oy, -ox + n, -oy + h)
            cut = self._cut(key, side, box, fade)
            if cut is None: return -1
            rgb, shown, mask = cut
            return ncc_pooled(crop, Image.composite(rgb, card, shown) if fade else rgb, mask)
        first = sorted(((score(key, 1.0, 0, 0), key) for key in self.heads), reverse=True)[:12]
        best = {}
        for _, key in first:
            s = max(score(key, sc, dx, dy) for sc in (0.95, 1.0, 1.05) for dx in (-2, -1, 0, 1, 2) for dy in (-2, -1, 0, 1, 2))
            name = self.heads[key][0]
            if s > best.get(name, (-2,))[0]: best[name] = (s, key)
        ranked = sorted(best.items(), key=lambda kv: -kv[1][0])
        if not ranked: return None
        (name, (s, key)) = ranked[0]
        return name, s, s - (ranked[1][1][0] if len(ranked) > 1 else -1), key

    # -- the Party screen's skill badges ------------------------------------------------------------------------------
    _badge_cache = {}

    @classmethod
    def _badge(cls, r):
        """A skill badge's look at radius r px (its dark round, 84 units): the dark ring between the glyph and the
        edge, and the light rim round it. The glyph inside differs per Aniimo, so it's left out."""
        k = round(r * 2) / 2
        if k not in cls._badge_cache:
            n = int(k * 1.2) * 2 + 1; c = n // 2
            img = Image.new('RGB', (n, n), (128, 128, 128)); mask = Image.new('L', (n, n), 0)
            di, dm = ImageDraw.Draw(img), ImageDraw.Draw(mask)
            ring = lambda f, t, col: (di.ellipse((c - k * t, c - k * t, c + k * t, c + k * t), fill=col),
                                      dm.ellipse((c - k * t, c - k * t, c + k * t, c + k * t), fill=255))
            ring(None, 1.07, (220, 220, 220)); ring(None, 0.97, (60, 60, 75))
            di.ellipse((c - k * 0.7, c - k * 0.7, c + k * 0.7, c + k * 0.7), fill=(128, 128, 128))
            dm.ellipse((c - k * 0.7, c - k * 0.7, c + k * 0.7, c + k * 0.7), fill=0)
            cls._badge_cache[k] = (img, mask, c)
        return cls._badge_cache[k]

    def badge(self, im, x, y, r, reach, reach_y=None, step=2):
        """The centre of the skill badge nearest (x, y) (screen px, its round r px across half), searched `reach`
        px around (`reach_y` up and down, when given): (score, x, y). Every `step` px, then the best to 1 px."""
        img, mask, c = self._badge(r)
        n = img.width
        ry = reach if reach_y is None else reach_y
        def at(px, py):
            x0, y0 = int(round(px)) - c, int(round(py)) - c
            return ncc(im.crop((x0, y0, x0 + n, y0 + n)).convert('RGB'), img, mask)
        best = max((at(x + dx, y + dy), x + dx, y + dy) for dx in range(-reach, reach + 1, step) for dy in range(-ry, ry + 1, step))
        for _ in range(2 if step > 2 else 1):
            best = max((at(best[1] + dx, best[2] + dy), best[1] + dx, best[2] + dy) for dx in (-1, 0, 1) for dy in (-1, 0, 1))
        return best

    # -- Holo-Battle Sim ---------------------------------------------------------------------------------------------
    def sim_glyph(self, im, box):
        """The series whose white glyph shows in `box` (one icon of the Victory screen's Holo-gizmo row), by shape:
        (series id, overlap 0..1, margin over the next), or None for an empty slot. Each glyph and icon is cropped to
        its own outline and compared at one size, so the icon's drawn size doesn't matter."""
        crop = im.crop(tuple(int(v) for v in box)).convert('RGB')
        r, g, b = crop.split()
        low = ImageChops.darker(ImageChops.darker(r, g), b); high = ImageChops.lighter(ImageChops.lighter(r, g), b)
        white = ImageChops.multiply(low.point(lambda v: 255 if v > 200 else 0), ImageChops.subtract(high, low).point(lambda v: 255 if v < 40 else 0))
        def outline(m):
            m = m.point(lambda v: 255 if v > 128 else 0); bb = m.getbbox()
            return m.crop(bb).resize((48, 48)).point(lambda v: 255 if v > 128 else 0) if bb else None
        shape = outline(white)
        if shape is None or shape.histogram()[255] < 200: return None
        res = []
        for (part, sid), art in self.sim_art.items():
            if part != 'series': continue
            ref = outline(art.getchannel('A'))
            if ref is None: continue
            both = ImageChops.darker(shape, ref).histogram()[255]; either = ImageChops.lighter(shape, ref).histogram()[255]
            res.append((both / either if either else 0, sid))
        res.sort(reverse=True)
        if not res: return None
        return res[0][1], res[0][0], res[0][0] - (res[1][0] if len(res) > 1 else 0)

    def sim_picture(self, im, box, part):
        """The series whose `part` picture ('core': the Core gizmo, 'transform': the Alpha Transformation) shows in
        `box` (its round frame on the Victory screen): (series id, score, margin), or None. The picture is drawn
        smaller than the frame over a coloured disk, so it's fitted at a few sizes and offsets, compared only where
        the picture itself is opaque."""
        N = 80
        crop = im.crop(tuple(int(v) for v in box)).convert('RGB').resize((N, N), Image.BILINEAR)
        res = []
        for (p, sid), art in self.sim_art.items():
            if p != part: continue
            best = -1
            if part == 'transform':   # the portrait's head end fills the round frame: compared inside the circle
                for inset in (0, 4, 8):
                    side = N - 2 * inset
                    mask = Image.new('L', (N, N), 0); ImageDraw.Draw(mask).ellipse((inset + 4, inset + 4, N - inset - 4, N - inset - 4), fill=255)
                    bg = Image.new('RGB', art.size, (128, 128, 128)); bg.paste(art, (0, 0), art)
                    ref = Image.new('RGB', (N, N), (128, 128, 128)); ref.paste(bg.resize((side, side), Image.BILINEAR), (inset, inset))
                    best = max(best, ncc(crop, ref, mask))
                res.append((best, sid)); continue
            for side in (52, 58, 64, 70):
                g = art.copy(); g.thumbnail((side, side), Image.BILINEAR)
                mask = g.getchannel('A').point(lambda v: 255 if v > 160 else 0); rgb = g.convert('RGB')
                for dx in range(-8, 9, 4):
                    for dy in range(-8, 9, 4):
                        x = (N - g.width) // 2 + dx; y = (N - g.height) // 2 + dy
                        if x < 0 or y < 0 or x + g.width > N or y + g.height > N: continue
                        best = max(best, ncc(crop.crop((x, y, x + g.width, y + g.height)), rgb, mask))
            res.append((best, sid))
        res.sort(reverse=True)
        if not res: return None
        return res[0][1], res[0][0], res[0][0] - (res[1][0] if len(res) > 1 else -1)

    # -- buffs ------------------------------------------------------------------------------------------------------
    def buff(self, im, box, kind, text_below=True):
        """The buff of `kind` ('season' / 'offering') whose white glyph shows in `box` (its icon's square), as
        (icon, score, margin), or None. The glyph is compared as a shape: the white in the box against the icon's alpha,
        its whole picture, padding and all, at the size the game draws it. Measured on real 4K screens (8 Nov): a Season
        Buff's about 1.03 of the box (its "Lv. N" overlaps below, so only the top is compared), a Special Offering's 0.84
        to 0.91, in a circle whose white ring and arrow, on a highlighted one, are left out of the comparison."""
        N = BUFF_SIZE
        crop = im.crop(tuple(int(v) for v in box)).convert('RGB').resize((N, N), Image.BILINEAR)
        r, g, b = crop.split()
        white = ImageChops.darker(ImageChops.darker(r, g), b)       # bright in every channel: the glyph, not the blue
        white = Image.merge('RGB', (white,) * 3)
        mask = Image.new('L', (N, N), 0)
        if text_below: ImageDraw.Draw(mask).rectangle((0, 0, N, int(N * 0.62)), fill=255)
        else: ImageDraw.Draw(mask).ellipse((N * 0.1, N * 0.1, N * 0.9, N * 0.9), fill=255)
        sides, dxs, dys = ((0.98, 1.08), range(-2, 3, 2), range(-4, 7, 2)) if text_below else ((0.8, 0.96), range(-2, 5, 2), range(-4, 9, 2))
        res = []
        for icon in {b['icon'] for b in self.buffs.values() if b['kind'] == kind}:
            best = -1
            for side in range(int(N * sides[0]), int(N * sides[1]) + 1, 2):
                g = self.buff_art[icon].resize((side, side), Image.BILINEAR)
                x0 = (N - side) // 2
                for dx in dxs:
                    for dy in dys:
                        ref = Image.new('L', (N, N), 0); ref.paste(g, (x0 + dx, x0 + dy))
                        best = max(best, ncc(white, Image.merge('RGB', (ref,) * 3), mask))
            res.append((best, icon))
        res.sort(reverse=True)
        if not res: return None
        return res[0][1], res[0][0], res[0][0] - (res[1][0] if len(res) > 1 else -1)

    def buffs_named(self, icon, kind, level=None):
        """The buff ids for an icon (Season Buffs: one per level; a few Special Offerings share an icon)."""
        return [k for k, b in self.buffs.items() if b['icon'] == icon and b['kind'] == kind and (level is None or b.get('level') == level)]

    # -- "Lv. N" ------------------------------------------------------------------------------------------------------
    def digit(self, im, box):
        """A lone white digit in `box` (the N of a Season Buff's "Lv. N"), by its shape against drawn digits:
        (digit, distance, margin) or None. OCR misses these: too small and alone."""
        if self._digits is None:
            self._digits = {}
            for fn in ('arialbd.ttf', 'seguisb.ttf', 'segoeui.ttf', 'arial.ttf'):
                try: font = ImageFont.truetype(fn, 60)
                except OSError: continue
                for d in '123456789':
                    g = Image.new('L', (60, 80), 0); ImageDraw.Draw(g).text((8, 0), d, fill=255, font=font)
                    self._digits.setdefault(d, []).append(_shape(g.point(lambda v: 255 if v > 128 else 0)))
        crop = im.crop(tuple(int(v) for v in box)).convert('RGB')
        r, g, b = crop.split()
        low = ImageChops.darker(ImageChops.darker(r, g), b); high = ImageChops.lighter(ImageChops.lighter(r, g), b)
        grey = ImageChops.subtract(high, low).point(lambda v: 255 if v < 45 else 0)          # not coloured
        mask = ImageChops.multiply(low.point(lambda v: 255 if v > 200 else 0), grey)       # and bright: white
        top = int(mask.height * 0.18); ImageDraw.Draw(mask).rectangle((0, 0, mask.width, top), fill=0)
        shape = _shape(mask)
        if shape is None: return None
        res = sorted((min(ImageStat.Stat(ImageChops.difference(shape, t)).mean[0] / 255 for t in ts), d) for d, ts in self._digits.items())
        return res[0][1], res[0][0], res[1][0] - res[0][0]


def _shape(mask):
    """A glyph's white pixels, cropped to them and set to a fixed size (None when there's hardly anything)."""
    bbox = mask.getbbox()
    if not bbox or (bbox[2] - bbox[0]) * (bbox[3] - bbox[1]) < 8: return None
    return mask.crop(bbox).resize((14, 20), Image.BILINEAR)
