"""The game's own HUD font for the overlay text.

Aniimo draws its UI text with TextMeshPro signed-distance-field fonts. The English one (UI_Font_Europe, HYZhengYuan)
is read once from the player's own game install (never shipped with aLogs) and cached in the data folder as an atlas
PNG plus a glyph table. Text is then drawn like the game does: the distance field gives smooth edges at any size, a
thin dark outline and a soft drop shadow, optionally slanted for the HUD's italic numbers.
"""
import json, os, re
from PIL import Image, ImageChops, ImageFilter

FONT_NAME = 'UI_Font_Europe'
CACHE_VERSION = 1

def _game_dirs(player_log=None):
    """Aniimo's StreamingAssets bundle folder: from the game's own log (it prints its install path), else Steam."""
    candidates = []
    if player_log and os.path.exists(player_log):
        try:
            with open(player_log, 'rb') as f: head = f.read(400_000).decode('utf-8', 'replace')
            for m in re.finditer(r'([A-Za-z]:[/\\][^\r\n"]*?steamapps[/\\]common[/\\]Aniimo)', head):
                candidates.append(m.group(1))
        except OSError: pass
    for drive in 'CDEFG':
        for base in (r'\Program Files (x86)\Steam', r'\Program Files\Steam', r'\SteamLibrary', r'\Steam'):
            candidates.append(drive + ':' + base + r'\steamapps\common\Aniimo')
    seen = set()
    for c in candidates:
        d = os.path.join(c.replace('/', '\\'), 'Aniimo_Data', 'StreamingAssets', 'cvs', 'res', 'uab', 'win', 'DefaultPackage')
        if d.lower() not in seen and os.path.isdir(d):
            seen.add(d.lower()); yield d

def extract(cache_dir, player_log=None):
    """Writes <cache_dir>/gamefont.png and gamefont.json from the game install. Returns True on success."""
    import UnityPy                                     # only needed this once
    for d in _game_dirs(player_log):
        bundles = [os.path.join(d, n) for n in os.listdir(d) if n.startswith('xgui_font')]
        if not bundles: continue
        env = UnityPy.load(*bundles)
        for obj in env.objects:
            if obj.type.name != 'MonoBehaviour': continue
            tree = obj.read_typetree()
            if tree.get('m_Name') != FONT_NAME: continue
            atlas_ref = (tree.get('m_AtlasTextures') or [None])[0]
            tex = None
            for o in env.objects:
                if o.type.name == 'Texture2D' and o.path_id == atlas_ref.get('m_PathID'):
                    tex = o.read(); break
            if tex is None: continue
            os.makedirs(cache_dir, exist_ok=True)
            img = tex.image.convert('RGBA').split()[-1]          # the distance field is in alpha
            img.save(os.path.join(cache_dir, 'gamefont.png'))
            face = tree['m_FaceInfo']
            glyphs = {g['m_Index']: g for g in tree['m_GlyphTable']}
            table = {}
            for ch in tree['m_CharacterTable']:
                g = glyphs.get(ch['m_GlyphIndex'])
                if not g: continue
                r, m = g['m_GlyphRect'], g['m_Metrics']
                table[ch['m_Unicode']] = [r['m_X'], r['m_Y'], r['m_Width'], r['m_Height'],
                                          m['m_HorizontalBearingX'], m['m_HorizontalBearingY'], m['m_HorizontalAdvance']]
            meta = {'version': CACHE_VERSION, 'point_size': face['m_PointSize'], 'ascent': face['m_AscentLine'],
                    'descent': face['m_DescentLine'], 'line_height': face['m_LineHeight'], 'padding': tree.get('m_AtlasPadding', 2),
                    'atlas_height': img.height, 'glyphs': table}
            with open(os.path.join(cache_dir, 'gamefont.json'), 'w', encoding='utf-8') as f: json.dump(meta, f)
            return True
    return False

class GameFont:
    def __init__(self, cache_dir):
        with open(os.path.join(cache_dir, 'gamefont.json'), encoding='utf-8') as f: self.meta = json.load(f)
        if self.meta.get('version') != CACHE_VERSION: raise ValueError('old font cache')
        self.atlas = Image.open(os.path.join(cache_dir, 'gamefont.png')).convert('L')
        self.glyphs = {int(k): v for k, v in self.meta['glyphs'].items()}
        self.cache = {}

    @classmethod
    def load(cls, cache_dir, player_log=None):
        """The cached font, extracting it from the game first if needed; None when the game files can't be read."""
        try: return cls(cache_dir)
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

    def _field(self, text, px):
        """The text's distance field (0..255, 128 = the glyph edge) at px font size, plus the baseline y."""
        m = self.meta; s = px / m['point_size']; pad = m['padding']
        spread = 10                                     # room around the text for the outline and shadow
        width = sum(self.glyphs.get(ord(c), self.glyphs.get(63))[6] for c in text) * s
        asc, desc = m['ascent'] * s, -m['descent'] * s
        W, H = int(width + 2 * spread * s + 4), int(asc + desc + 2 * spread * s + 4)
        field = Image.new('L', (W, H), 0)
        x = spread * s; base = spread * s + asc; AH = m['atlas_height']
        for c in text:
            g = self.glyphs.get(ord(c)) or self.glyphs.get(63)
            gx, gy, gw, gh, bx, by, adv = g
            if gw and gh:
                # atlas rects are bottom-up (Unity); include the padding, which holds the field's falloff
                box = (gx - pad, AH - (gy + gh) - pad, gx + gw + pad, AH - gy + pad)
                crop = self.atlas.crop(box)
                cw, ch = max(1, round(crop.width * s)), max(1, round(crop.height * s))
                crop = crop.resize((cw, ch), Image.BICUBIC)
                px0, py0 = round(x + (bx - pad) * s), round(base - (by + pad) * s)
                region = (px0, py0, px0 + cw, py0 + ch)
                field.paste(ImageChops.lighter(field.crop(region), crop), region)
            x += adv * s
        return field, base

    def render(self, text, px, fill=(255, 255, 255), outline=(20, 26, 44), italic=False, bold=False):
        """RGBA image of the text in the game's style (bold thickens the strokes like the HUD's numbers)."""
        key = (text, px, fill, outline, italic, bold)
        if key in self.cache: return self.cache[key]
        field, base = self._field(text, px)
        soft = max(1.0, 64 / max(px, 1) * 2)            # edge softness in field units (sharper at larger sizes)
        def band(edge):
            return field.point(lambda v: int(max(0, min(255, (v - edge) * 255 / (2 * soft * 8) + 128))))
        edge = 128 - (14 if bold else 0)
        body = band(edge)                                      # the letters
        rim = band(edge - 22)                                  # a thin outline
        shadow = rim.filter(ImageFilter.GaussianBlur(max(1.5, px / 10)))     # soft, like the game's text shadow
        W, H = field.size; dy = max(1, round(px / 20))
        out = Image.new('RGBA', (W, H + dy), (0, 0, 0, 0))
        sh = Image.new('RGBA', (W, H), outline + (0,)); sh.putalpha(shadow.point(lambda v: v * 120 // 255))
        out.alpha_composite(sh, (0, dy))
        ol = Image.new('RGBA', (W, H), outline + (0,)); ol.putalpha(rim.point(lambda v: v * 55 // 255))                  # barely-there outline
        out.alpha_composite(ol)
        fg = Image.new('RGBA', (W, H), fill + (0,)); fg.putalpha(body)
        out.alpha_composite(fg)
        if italic:                                            # the HUD's numbers lean ~12 degrees
            k = 0.21; out = out.transform((out.width + int(out.height * k), out.height), Image.AFFINE,
                                          (1, k, -out.height * k, 0, 1, 0), Image.BICUBIC)
        bbox = out.getbbox() or (0, 0, 1, 1)
        out = out.crop(bbox)
        self.cache[key] = out
        if len(self.cache) > 300: self.cache.pop(next(iter(self.cache)))
        return out
