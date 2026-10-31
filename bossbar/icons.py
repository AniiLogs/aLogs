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

CACHE_VERSION = 2          # 2: Holo-Battle Sim's series, Core gizmo and Alpha Transformation pictures
HEAD_SIZE = 96              # cached head portraits (px square)
BUFF_SIZE = 64
SIM_SIZE = 96               # cached Holo-Battle Sim pictures (longest side)
BUFF_PREFIXES = ('mres_xgui_texture_expng_', 'mres_xgui_panel_expng_', 'mres_xgui_common_expng_', 'ar_xgui_icon_skilli_',
                 'ar_xgui_icon_item_i_')
HEAD_PREFIXES = ('ar_xgui_image_pet_he_', 'ar_xgui_image_petman_')
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

def extract(cache_dir, player_log=None):
    """Writes <cache_dir>/heads/*.png, buffs/*.png and icons.json from the game install. True on success."""
    import UnityPy, gamedata
    pm_path, game = _game(player_log)
    if not game: return False
    pm, en = gamedata.PmData(pm_path), gamedata.english_text(pm_path)
    buff_rows = {k: r for k, r in (pm.table('bossrush_buff_data') or {}).items() if r.get('buffType') in KIND}
    icon_of = lambda r: (r.get('buffIcon') or '').lstrip('$').rsplit('.', 1)[0]
    wanted = {icon_of(r).lower() for r in buff_rows.values()}
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
    shipped = sorted(f for f in glob.glob(os.path.join(_bundle_dir(game), '*.uab'))
                     if os.path.basename(f).startswith(BUFF_PREFIXES + HEAD_PREFIXES))
    patches = glob.glob(os.path.join(game, 'Aniimo_Data', 'cvs', 'res', 'uab', 'win', 'DefaultPackage', 'CacheBundleFiles', '*', '*', 'cdata.uab'))
    art = {}                                                   # lower name -> (image, file time); the newest file wins
    for f in shipped + patches:
        try: env = UnityPy.load(f); stamp = os.path.getmtime(f)
        except Exception: continue
        for o in env.objects:
            if o.type.name != 'Sprite': continue
            name = o.peek_name() or ''
            low = name.lower()
            if (low in wanted or low.startswith('ui_pethead_')) and (low not in art or art[low][1] < stamp):
                try: art[low] = (o.read().image.convert('RGBA'), stamp)
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
        icon = low[len('ui_pethead_'):].split('_')[0]
        own = [n for p, n in by_icon.get(icon, []) if p in (icon, icon + '00')]
        names = [n for _, n in by_icon.get(icon, [])]
        name = own[0] if own else (max(set(names), key=names.count) if names else None)
        if not name: continue
        sq = Image.new('RGBA', (max(img.size),) * 2)               # the portrait square (a few are trimmed)
        sq.paste(img, ((sq.width - img.width) // 2, sq.height - img.height))
        sq.resize((HEAD_SIZE, HEAD_SIZE), Image.LANCZOS).save(os.path.join(cache_dir, 'heads', icon + '.png'))
        heads[icon] = name
    buffs = {}
    for k, r in buff_rows.items():
        low = icon_of(r).lower()
        if low not in art: continue
        art[low][0].resize((BUFF_SIZE, BUFF_SIZE), Image.LANCZOS).save(os.path.join(cache_dir, 'buffs', low + '.png'))
        buffs[str(k)] = {'name': en(r.get('buffName')), 'kind': KIND[r['buffType']], 'icon': low,
                         **({'level': int(r['Bufflevel'])} if r.get('Bufflevel') else {})}
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
        json.dump({'version': CACHE_VERSION, 'build': _build(game), 'heads': heads, 'buffs': buffs, 'sim': sim_out}, f, ensure_ascii=False)
    return bool(heads and buffs)


def ncc(a, b, mask):
    """Masked normalized correlation of two same-size RGB images, averaged over the channels (-1..1)."""
    sa, sb = ImageStat.Stat(a, mask), ImageStat.Stat(b, mask)
    if not sa.count[0]: return -1
    cross = ImageStat.Stat(ImageChops.multiply(a, b), mask).mean
    out = 0
    for k in range(3):
        cov = cross[k] * 255 - sa.mean[k] * sb.mean[k]
        out += cov / ((sa.var[k] * sb.var[k]) ** 0.5 + 1e-6)
    return out / 3


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
        self.sim_art = {}             # (part, series id) -> RGBA picture
        for sid, s in self.sim.items():
            for part in ('series', 'core', 'transform'):
                if s.get(part + '_art'):
                    self.sim_art[(part, sid)] = Image.open(os.path.join(cache_dir, 'sim', f'{part}_{sid}.png')).convert('RGBA')
        self._sized = {}
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

    def head(self, im, window, square, n=40):
        """The Aniimo whose portrait shows through `window` (a screen box), the portrait's whole square being about
        `square` (x, y, side in screen px; it's cut by the window). (name, score, margin over the next name, icon), or None.
        A first pass with the square as placed picks a dozen candidates, a second fits each a few px and % around."""
        X0, Y0, X1, Y1 = window
        h = max(4, round(n * (Y1 - Y0) / (X1 - X0))); k = n / (X1 - X0)
        crop = im.crop(tuple(int(v) for v in window)).convert('RGB').resize((n, h), Image.BILINEAR)
        side0 = square[2] * k; ox0 = (square[0] - X0) * k; oy0 = (square[1] - Y0) * k
        def score(key, scale, dx, dy):
            side = int(side0 * scale)
            ox = int(round(ox0 + (side0 - side) / 2)) + dx; oy = int(round(oy0 + (side0 - side) / 2)) + dy
            if ox > 0 or oy > 0 or ox + side < n or oy + side < h: return -1
            part = self._head(key, side).crop((-ox, -oy, -ox + n, -oy + h))
            mask = part.getchannel('A').point(lambda v: 255 if v > 200 else 0)
            if ImageStat.Stat(mask).mean[0] < 0.3 * 255: return -1
            return ncc(crop, part.convert('RGB'), mask)
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
        drawn about two thirds the box's size (a Season Buff's sits a little high, its "Lv. N" overlapping below)."""
        N = BUFF_SIZE
        crop = im.crop(tuple(int(v) for v in box)).convert('RGB').resize((N, N), Image.BILINEAR)
        r, g, b = crop.split()
        white = ImageChops.darker(ImageChops.darker(r, g), b)       # bright in every channel: the glyph, not the blue
        white = Image.merge('RGB', (white,) * 3)
        mask = Image.new('L', (N, N), 0); ImageDraw.Draw(mask).rectangle((0, 0, N, int(N * (0.62 if text_below else 1))), fill=255)
        res = []
        for icon in {b['icon'] for b in self.buffs.values() if b['kind'] == kind}:
            best = -1
            for side in range(int(N * 0.6), int(N * 0.76), 2):
                g = self.buff_art[icon].resize((side, side), Image.BILINEAR)
                x0 = (N - side) // 2
                for ox in range(x0 - 3, x0 + 4, 2):
                    for oy in range(int(N * 0.12), x0 + 4, 2):
                        ref = Image.new('L', (N, N), 0); ref.paste(g, (ox, oy))
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
