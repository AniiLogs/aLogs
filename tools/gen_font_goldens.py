"""Goldens for gamefont.py's field and render (aLogs B16) on a synthetic font cache made here (no game files).

    python -I tools/gen_font_goldens.py [reference/aLogs/bossbar]
"""
import hashlib, json, os, random, sys
here = os.path.dirname(os.path.abspath(__file__))
src = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, "..", "reference", "aLogs", "bossbar"))
sys.path.insert(0, src)
from PIL import Image, ImageDraw, ImageFilter
import gamefont as G
OUT = os.path.join(here, "..", "crates", "alogs-reader", "tests", "data", "font")
os.makedirs(OUT, exist_ok=True)
rnd = random.Random(3)
atlas = Image.new('L', (256, 256), 0); d = ImageDraw.Draw(atlas)
chars = [c for c in "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789?%/.,:K"]
glyphs = {}; x = y = 4
for c in dict.fromkeys(chars):
    gw, gh = rnd.randint(6, 12), rnd.randint(10, 16)
    if x + gw + 6 > 256: x = 4; y += 22
    for _ in range(3):
        d.ellipse((x + rnd.randint(0, gw // 2), y + rnd.randint(0, gh // 2), x + gw, y + gh), fill=rnd.randint(150, 255))
    glyphs[ord(c)] = [x, 256 - (y + gh), gw, gh, rnd.randint(0, 2), gh - rnd.randint(0, 3), gw + rnd.randint(1, 3)]
    x += gw + 6
glyphs[32] = [0, 0, 0, 0, 0, 0, 5]
atlas = atlas.filter(ImageFilter.GaussianBlur(1.5))
atlas.save(os.path.join(OUT, 'gamefont.png'))
meta = {'version': G.CACHE_VERSION, 'point_size': 16, 'ascent': 14, 'descent': -4, 'line_height': 20, 'padding': 2,
        'atlas_height': 256, 'glyphs': {str(k): v for k, v in glyphs.items()}}
json.dump(meta, open(os.path.join(OUT, 'gamefont.json'), 'w'))
f = G.GameFont(OUT)
md5 = lambda im: hashlib.md5(im.tobytes()).hexdigest()
cases = []
texts = ["Kyhra", "12,345", "88.8K / 8.88M HP", "HP 99%", "Last 5s", "x? Q", "BREAK"]
for t in texts:
    for px in (11, 17, 24, 40):
        fld, base = f._field(t, px)
        cases.append({"kind": "field", "text": t, "px": px, "size": list(fld.size), "md5": md5(fld), "base": base})
        for italic, bold, stroke, lead in ((False, False, False, None), (True, False, False, None), (False, True, True, None),
                                           (False, False, True, (2, (255, 80, 60))), (True, True, False, None)):
            im = f.render(t, px, (255, 230, 120), italic=italic, bold=bold, stroke=stroke, lead=lead)
            cases.append({"kind": "render", "text": t, "px": px, "italic": italic, "bold": bold, "stroke": stroke,
                          "lead": [lead[0], list(lead[1])] if lead else None, "size": list(im.size), "md5": md5(im), "base": im.info['base']})
json.dump(cases, open(os.path.join(OUT, 'font_goldens.json'), 'w'))
print("font goldens:", len(cases))
