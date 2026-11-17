"""Goldens for alogs-pil's draw::polygon (crates/alogs-pil/tests/bcn.rs): ImageDraw.polygon(fill=1) on mode "1"
images, the way UnityPy masks a sprite by its mesh's triangles. Made-up float triangles and quads.
    python tools/gen_polygon_goldens.py   (writes crates/alogs-pil/tests/data/polygon_goldens.json)
"""
import hashlib, json, os, random
from PIL import Image, ImageDraw

random.seed(5)
out = []
for k in range(300):
    w, h = random.randint(1, 60), random.randint(1, 60)
    tris = []
    for _ in range(random.randint(1, 6)):
        if k % 3 == 0:     # sprite-like: vertices near the box's corners and edges
            pts = [(random.choice([0, w, random.uniform(0, w)]), random.choice([0, h, random.uniform(0, h)])) for _ in range(3)]
        else:
            pts = [(random.uniform(-5, w + 5), random.uniform(-5, h + 5)) for _ in range(3)]
        tris.append([[round(x, 4), round(y, 4)] for x, y in pts])
    im = Image.new('1', (w, h), 0)
    d = ImageDraw.ImageDraw(im)
    for t in tris: d.polygon([tuple(p) for p in t], fill=1)
    out.append({'size': [w, h], 'tris': tris, 'md5': hashlib.md5(im.convert('L').tobytes()).hexdigest()})
path = os.path.join(os.path.dirname(__file__), '..', 'crates', 'alogs-pil', 'tests', 'data', 'polygon_goldens.json')
with open(path, 'w') as f: json.dump(out, f)
print(len(out), 'cases')
