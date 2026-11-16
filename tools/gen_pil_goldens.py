"""Writes crates/alogs-pil/tests/data/pil_goldens.json: Pillow's results (a hash of mode, size and bytes) for each
operation aLogs' readers use, on images both sides make from a seed (see make() here and in tests/pil.rs).

    python -I tools/gen_pil_goldens.py
"""
import hashlib, json, math, os, random
from PIL import Image, ImageChops, ImageFilter, ImageOps, ImageStat, ImageDraw

M64 = (1 << 64) - 1


def make(mode, w, h, seed, kind):
    s = (seed * 2654435761 + 1) & M64
    bands = {"1": 1, "L": 1, "RGB": 3, "RGBA": 4}[mode]
    out = bytearray()

    def nxt():
        nonlocal s
        s = (s * 6364136223846793005 + 1442695040888963407) & M64
        return s >> 56
    for y in range(h):
        for x in range(w):
            for b in range(bands):
                r = nxt()
                if kind == 0: v = r
                elif kind == 1: v = (x * 7 + y * 13 + b * 40 + (r >> 4)) & 255
                elif kind == 2: v = nxt() if r < 40 else 0
                else: v = 255 if (x // 3 + y // 2 + b) % 3 == 0 or r > 230 else 0
                out.append(v)
    if mode == "1":
        im = Image.frombytes("L", (w, h), bytes(out)).point(lambda v: 255 if v >= 128 else 0)
        return im.convert("1", dither=Image.Dither.NONE)
    return Image.frombytes(mode, (w, h), bytes(out))


def digest(im):
    h = hashlib.sha256(f"{im.mode} {im.width} {im.height} ".encode() + im.tobytes()).hexdigest()[:20]
    return [im.mode, im.width, im.height, h]


cases = []
rnd = random.Random(11)


def case(op, args, inputs, fn):
    ims = [make(*i) for i in inputs]
    try: out = fn(*ims)
    except Exception as ex: out = ex
    if isinstance(out, Exception): res = {"error": type(out).__name__}
    elif isinstance(out, Image.Image): res = {"image": digest(out)}
    else: res = {"value": out}
    cases.append({"op": op, "args": args, "inputs": inputs, **res})


MODES = ["1", "L", "RGB", "RGBA"]
FILTERS = {"nearest": Image.NEAREST, "box": Image.BOX, "bilinear": Image.BILINEAR, "hamming": Image.HAMMING,
           "bicubic": Image.BICUBIC, "lanczos": Image.LANCZOS}
src = lambda m, k=None: [m, rnd.randint(1, 40), rnd.randint(1, 30), rnd.randint(0, 10**6), rnd.randint(0, 3) if k is None else k]

for _ in range(6):
    for a in MODES:
        for b in MODES:
            case("convert", {"mode": b}, [src(a)], lambda im, b=b: im.convert(b))
for _ in range(20):
    m = rnd.choice(MODES); t = rnd.randint(0, 255)
    case("point_threshold", {"t": t}, [src(m)], lambda im, t=t: im.point(lambda v: 255 if v > t else 0))
    case("point_scale", {"k": 1.7}, [src(m)], lambda im: im.point(lambda v: v * 1.7 - 20))
    case("point_to_1", {"t": t}, [src("L")], lambda im, t=t: im.point(lambda v: 255 if v > t else 0, "1"))
for _ in range(30):
    m = rnd.choice(MODES)
    box = [rnd.uniform(-10, 30), rnd.uniform(-10, 25), rnd.uniform(0, 50), rnd.uniform(0, 40)]
    box = [round(v * 2) / 2 for v in box]
    case("crop", {"box": box}, [src(m)], lambda im, box=box: im.crop(tuple(box)))
for _ in range(60):
    m = rnd.choice(MODES); f = rnd.choice(list(FILTERS))
    w, h = rnd.randint(1, 70), rnd.randint(1, 50)
    case("resize", {"w": w, "h": h, "filter": f}, [src(m)], lambda im, w=w, h=h, f=f: im.resize((w, h), FILTERS[f]))
for _ in range(15):
    m = rnd.choice(["L", "RGB", "RGBA"]); f = rnd.choice(list(FILTERS))
    w, h = rnd.randint(1, 30), rnd.randint(1, 30)
    case("resize_default", {"w": w, "h": h}, [src(m)], lambda im, w=w, h=h: im.resize((w, h)))
    i = src(m); bx = [rnd.uniform(0, 5), rnd.uniform(0, 5), rnd.uniform(6, i[1] + 2), rnd.uniform(6, i[2] + 2)]
    case("resize_box", {"w": w, "h": h, "filter": f, "box": bx}, [i],
         lambda im, w=w, h=h, f=f, bx=bx: im.resize((w, h), FILTERS[f], box=tuple(bx)))
    g = rnd.choice([1.0, 2.0, 3.0])
    case("resize_gap", {"w": w // 3 + 1, "h": h // 3 + 1, "filter": f, "gap": g}, [[m, 80, 60, rnd.randint(0, 999), 1]],
         lambda im, w=w // 3 + 1, h=h // 3 + 1, f=f, g=g: im.resize((w, h), FILTERS[f], reducing_gap=g))
for _ in range(30):
    m = rnd.choice(["L", "RGB", "RGBA"]); k = rnd.randint(1, 9)
    case("reduce", {"f": k}, [src(m)], lambda im, k=k: im.reduce(k))
    fx, fy = rnd.randint(1, 6), rnd.randint(1, 6)
    case("reduce_xy", {"fx": fx, "fy": fy}, [[m, 37, 29, rnd.randint(0, 999), 0]], lambda im, fx=fx, fy=fy: im.reduce((fx, fy), box=(3, 2, 31, 27)))
for _ in range(15):
    m = rnd.choice(["L", "RGB", "RGBA"]); tw, th = rnd.randint(4, 60), rnd.randint(4, 60)

    def thumb(im, tw=tw, th=th):
        im = im.copy(); im.thumbnail((tw, th)); return im
    case("thumbnail", {"w": tw, "h": th}, [[m, rnd.randint(10, 120), rnd.randint(10, 90), rnd.randint(0, 999), 1]], thumb)
for _ in range(30):
    m = rnd.choice(MODES); f = rnd.choice(["nearest", "bilinear", "bicubic"])
    a = [rnd.uniform(0.3, 2), rnd.choice([0, rnd.uniform(-0.5, 0.5)]), rnd.uniform(-5, 5),
         rnd.choice([0, rnd.uniform(-0.5, 0.5)]), rnd.uniform(0.3, 2), rnd.uniform(-5, 5)]
    w, h = rnd.randint(1, 50), rnd.randint(1, 40)
    case("affine", {"w": w, "h": h, "a": a, "filter": f}, [src(m)],
         lambda im, w=w, h=h, a=a, f=f: im.transform((w, h), Image.AFFINE, tuple(a), FILTERS[f]))
for _ in range(10):
    m = rnd.choice(["L", "RGB"])
    case("rotate", {"angle": -11, "filter": "bicubic", "expand": True}, [src(m)],
         lambda im: im.rotate(-11, resample=Image.BICUBIC, expand=True, fillcolor=0))
    ang = rnd.choice([90, 270, 33.3, 180, 0, -45])
    case("rotate", {"angle": ang, "filter": "nearest", "expand": True}, [src(m)],
         lambda im, ang=ang: im.rotate(ang, expand=True))
for t in ["FLIP_LEFT_RIGHT", "FLIP_TOP_BOTTOM", "ROTATE_90", "ROTATE_180", "ROTATE_270", "TRANSPOSE", "TRANSVERSE"]:
    for m in MODES:
        case("transpose", {"t": t}, [src(m)], lambda im, t=t: im.transpose(getattr(Image.Transpose, t)))
for _ in range(12):
    for m in MODES:
        case("getbbox", {}, [src(m, 2)], lambda im: list(im.getbbox()) if im.getbbox() else None)
        case("histogram", {}, [src(m)], lambda im: im.histogram())
        case("extrema", {}, [src(m)], lambda im: [list(e) for e in im.getextrema()] if im.mode not in ("1", "L") else list(im.getextrema()))
for m in ["RGB", "RGBA"]:
    case("split_merge", {}, [src(m)], lambda im: Image.merge(im.mode, im.split()[::-1] if im.mode == "RGB" else im.split()))
for _ in range(20):
    a, b = rnd.choice(MODES), rnd.choice(MODES); mk = rnd.choice(["1", "L", "RGBA", None])
    i1, i2 = src(a), [b, 20, 20, rnd.randint(0, 999), rnd.randint(0, 3)]
    x, y = rnd.randint(-8, 15), rnd.randint(-8, 15)
    ins = [i2, i1] + ([[mk, i1[1], i1[2], rnd.randint(0, 999), rnd.randint(0, 3)]] if mk else [])

    def paste(base, im, mask=None, x=x, y=y):
        base = base.copy(); base.paste(im, (x, y), mask); return base
    case("paste", {"x": x, "y": y}, ins, paste)
for _ in range(10):
    m = rnd.choice(["L", "RGB", "RGBA"]); mk = rnd.choice(["1", "L"])
    case("composite", {}, [[m, 20, 15, rnd.randint(0, 999), 0], [m, 20, 15, rnd.randint(0, 999), 1], [mk, 20, 15, rnd.randint(0, 999), 1]],
         lambda a, b, c: Image.composite(a, b, c))
case("new", {}, [], lambda: Image.new("RGBA", (5, 3), (1, 2, 3, 4)))

dst = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "crates", "alogs-pil", "tests", "data", "pil_goldens.json")
os.makedirs(os.path.dirname(dst), exist_ok=True)
with open(dst, "w", encoding="utf-8", newline="\n") as f: json.dump(cases, f)
print(len(cases), "cases;", sum("error" in c for c in cases), "errors")
