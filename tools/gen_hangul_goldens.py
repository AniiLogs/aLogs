"""Goldens for hangulnames.py's font-free helpers (aLogs B16) on synthetic name boxes drawn here.

    python -I tools/gen_hangul_goldens.py [reference/aLogs/bossbar]
"""
import hashlib, json, os, random, sys
here = os.path.dirname(os.path.abspath(__file__))
src = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, "..", "reference", "aLogs", "bossbar"))
sys.path.insert(0, src)
from PIL import Image, ImageDraw, ImageFont
import hangulnames as K
OUT = os.path.join(here, "..", "crates", "alogs-reader", "tests", "data")
os.makedirs(os.path.join(OUT, "names"), exist_ok=True)
rnd = random.Random(9)
def digest(im):
    return None if im is None else [im.mode, im.width, im.height, hashlib.sha256(f"{im.mode} {im.width} {im.height} ".encode() + im.tobytes()).hexdigest()[:20]]
boxes = []
for i in range(14):
    w, h, pad = rnd.randint(80, 260), rnd.randint(30, 70), rnd.choice([0, 6, 24])
    im = Image.new("L", (w + 2 * pad, h + 2 * pad), 255); d = ImageDraw.Draw(im)
    if rnd.random() < 0.6: d.rounded_rectangle((pad - 3, pad + 2, pad + rnd.randint(10, 40), pad + h - 2), 8, fill=30)   # a pill at the edge
    font = ImageFont.load_default(size=int(h * 0.6))
    d.text((pad + rnd.randint(14, 30), pad + h * 0.15), rnd.choice(["Kyhra", "L", "ab_cd", "Mia 3P", "x", "O0", "Neg"]), fill=10, font=font)
    for _ in range(rnd.randint(0, 6)): x, y = rnd.randint(0, im.width - 3), rnd.randint(0, im.height - 3); d.rectangle((x, y, x + 1, y + 1), fill=0)
    if rnd.random() < 0.4: d.rectangle((pad + 5, pad, pad + w - 5, pad + 3), fill=20)    # a portrait's foot above
    name = f"name{i:02d}.png"; im.save(os.path.join(OUT, "names", name))
    t = K.tidy(im, pad)
    boxes.append({"file": name, "pad": pad, "tidy": digest(t), "pieces": K.ink_pieces(t), "pieces_raw": K.ink_pieces(im)})
replies = ["Kyhra\t\tKyhra", "O\t[[10, [[\"ㅇ\", 0.6], [\"O\", 0.3]]], [30, [[\"ㄱ\", 0.9]]]]\tO", "\t[[5, [[\"q\", 0.95]]]]\tq",
           "x", "a\tnot json\tb", "\t[]\t", "Neg③.\t\tNeg③.", "Soul _ Shadow①", "abc\t[[1, [[\"가\", 0.4]]], [9, [[\"a\", 0.9]]]]\t"]
out = {"boxes": boxes,
       "replies": [[r, list(K.split_reply(r)), K.hangul_first(K.split_reply(r)[1]), K.lone_latin(K.split_reply(r)[1]), K.looks_hangul(K.split_reply(r)[1])] for r in replies],
       "clean": [[t, K.clean_latin(t)] for t in ["Soul_ShadowXD①", "Neg③.", "a _ b", " x。 ", "", "❷abc❸", "name…"]]}
with open(os.path.join(OUT, "hangul_goldens.json"), "w", encoding="utf-8", newline="\n") as f: json.dump(out, f, ensure_ascii=False)
print(len(boxes), "boxes")
