"""Goldens for namereader.py's and icons.py's font-free parts (aLogs B16) on the synthetic frames and name boxes.

    python -I tools/gen_names_goldens.py [reference/aLogs/bossbar]
"""
import hashlib, json, os, random, sys
here = os.path.dirname(os.path.abspath(__file__))
src = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, "..", "reference", "aLogs", "bossbar"))
sys.path.insert(0, src)
import logging; logging.disable(logging.CRITICAL)
from PIL import Image
import namereader as N, icons as I
D = os.path.join(here, "..", "crates", "alogs-reader", "tests", "data")
def digest(im):
    return None if im is None else [im.mode, im.width, im.height, hashlib.sha256(f"{im.mode} {im.width} {im.height} ".encode() + im.tobytes()).hexdigest()[:20]]
rnd = random.Random(4); out = []
frames = sorted(os.listdir(os.path.join(D, "frames")))[:8]
for f in frames:
    im = Image.open(os.path.join(D, "frames", f)).convert("RGB"); W, H = im.size
    for _ in range(3):
        x0, y0 = rnd.randint(0, W // 2), rnd.randint(0, H // 6); box = (x0, y0, x0 + rnd.randint(60, W // 3), y0 + rnd.randint(20, H // 8))
        m = N.text_mask(im, box)
        ink, bg = N.ink_colour(im, box, dark=rnd.random() < 0.5)
        c = {"file": f, "box": list(box), "text_mask": digest(m), "characters_at": [list(x) for x in N.characters_at(m)] if m else None,
             "characters": [digest(p) for p in N.characters(m)] if m else None, "drop_icon": digest(N.drop_icon(m)) if m else None,
             "ink": list(ink) if ink else None}
        if ink: c["ink_mask"] = digest(N.ink_mask(im, box, ink, bg))
        parts = N.characters(m) if m else []
        if len(parts) >= 2: c["overlap"] = [N.overlap(parts[0], parts[1]), N.overlap(parts[0], parts[0], slack=1)]
        a = im.crop(box).convert("RGB"); b = im.crop((box[0] + 3, box[1] + 1, box[2] + 3, box[3] + 1)).convert("RGB")
        mask = a.convert("L").point(lambda v: 255 if v > 60 else 0)
        c["ncc"] = [I.ncc(a, b, mask), I.ncc_pooled(a, b, mask)]
        out.append(c)
with open(os.path.join(D, "names_goldens.json"), "w", encoding="utf-8", newline="\n") as fh: json.dump(out, fh)
print(len(out), "cases")
