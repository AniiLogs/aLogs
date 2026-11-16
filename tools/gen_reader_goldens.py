"""Synthetic boss-bar frames (drawn here: no game pictures) and what the reference reader.py / xn_digits.py (aLogs B16)
read from them, plus parse_* on made-up OCR text.

    python -I tools/gen_reader_goldens.py [reference/aLogs/bossbar]

Output: crates/alogs-reader/tests/data/frames/*.png and reader_goldens.json
"""
import hashlib, json, os, random, sys

here = os.path.dirname(os.path.abspath(__file__))
src = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, "..", "reference", "aLogs", "bossbar"))
sys.path.insert(0, src)
import logging
logging.disable(logging.CRITICAL)
from PIL import Image, ImageDraw, ImageFont  # noqa: E402
import reader as R  # noqa: E402
import xn_digits as X  # noqa: E402

OUT = os.path.join(here, "..", "crates", "alogs-reader", "tests", "data")
os.makedirs(os.path.join(OUT, "frames"), exist_ok=True)
rnd = random.Random(5)


def digest(im):
    return [im.mode, im.width, im.height, hashlib.sha256(f"{im.mode} {im.width} {im.height} ".encode() + im.tobytes()).hexdigest()[:20]]


def frame(W, H, fill, world=False, xn=12, ghost=0.05, gauge=0.6, gauge_kind="white", noise=30, decoy=True, cover=None):
    im = Image.new("RGB", (W, H))
    px = bytearray()
    for _ in range(W * H // 64 + 1):
        px += bytes(rnd.randint(10, 10 + noise) for _ in range(3))
    small = Image.frombytes("RGB", (W // 8 + 1, H // 8 + 1), bytes(px[:3 * (W // 8 + 1) * (H // 8 + 1)]).ljust(3 * (W // 8 + 1) * (H // 8 + 1), b"\x10"))
    im.paste(small.resize((W, H), Image.NEAREST))
    d = ImageDraw.Draw(im)
    U = min(W, H * 16 / 9)
    span = 0.3945 * U
    lb = W / 2 - span / 2
    yc = round(H * 0.07)
    t = max(3, round(span / 789 * 12))
    if decoy:   # a red-orange streak elsewhere near the top (fire), longer than a drained bar
        d.rectangle((int(W * 0.1), int(H * 0.15), int(W * 0.1 + span * 0.7), int(H * 0.15) + t), fill=(240, 140, 60))
    tl, tr = lb + 9 / 789 * span, lb + (784 / 789 if world else 716 / 789) * span
    d.rectangle((int(tl), yc - t // 2, int(tr), yc + t // 2), fill=(45, 32, 30))
    fx = tl + fill * (tr - tl)
    if fill > 0: d.rectangle((int(tl), yc - t // 2, int(fx) - 1, yc + t // 2), fill=(245, 150, 70))
    if ghost and fill < 1: d.rectangle((int(fx), yc - t // 2, int(fx + ghost * (tr - tl)), yc + t // 2), fill=(190, 80, 70))
    bw = max(1, round(span / 789 * 2))
    for x in (lb, lb + span):
        d.rectangle((int(x), yc - int(0.85 * t), int(x) + bw - 1, yc + int(0.85 * t)), fill=(250, 250, 250))
    if not world:
        bx0, bx1 = lb + 0.915 * span, lb + 0.99 * span
        d.rectangle((int(bx0), yc - int(0.7 * t), int(bx1), yc + int(0.7 * t)), fill=(25, 25, 35))
        font = ImageFont.load_default(size=max(8, int(1.4 * t)))
        d.text((int(bx0 + 0.15 * (bx1 - bx0)), yc - int(0.75 * t)), f"x{xn}", fill=(250, 250, 250), font=font)
    # break gauge: a dark rim band under the bar with the line inside it
    rim0, rim1 = yc + t // 2 + 1, int(yc + 0.03 * span)
    d.rectangle((int(tl), rim0, int(tr), rim1), fill=(20, 18, 18))
    ly = int(yc + 0.014 * span)
    col = (240, 240, 238) if gauge_kind == "white" else (245, 200, 150)
    if gauge > 0: d.rectangle((int(tl), ly, int(tl + gauge * (tr - tl)), ly + max(0, bw - 1)), fill=col)
    font = ImageFont.load_default(size=max(10, int(span / 789 * 30)))
    d.text((int(lb + 0.25 * span), int(yc - 0.05 * span)), "Lv.60 Young Omega Sherro", fill=(245, 245, 245), font=font)
    d.text((int(lb + 0.25 * span), int(yc - 0.05 * span)), "Lv.60", fill=(205, 55, 55), font=font) if rnd.random() < 0.3 else None
    if cover:   # one of the meter's own labels painted over the bar
        a, b = cover
        d.rectangle((int(tl + a * (tr - tl)), yc - t, int(tl + b * (tr - tl)), yc + t), fill=(255, 0, 255))
    return im


def jsonable(v):
    if isinstance(v, dict): return {k: jsonable(x) for k, x in v.items()}
    if isinstance(v, (list, tuple)): return [jsonable(x) for x in v]
    return v


TPL = X.load_templates()
cases = []
SIZES = [(1280, 720), (1600, 900), (1920, 1080), (2560, 1440), (3440, 1440), (3840, 2160)]
for i in range(26):
    W, H = SIZES[i % len(SIZES)]
    kw = dict(fill=rnd.choice([1.0, 0.83, 0.5, 0.21, 0.04, 0.0]), world=rnd.random() < 0.3, xn=rnd.choice([21, 12, 7, 3, 1]),
              ghost=rnd.choice([0, 0.04, 0.1]), gauge=rnd.choice([0.0, 0.3, 0.9]), gauge_kind=rnd.choice(["white", "orange"]),
              decoy=rnd.random() < 0.7, cover=rnd.choice([None, None, (0.3, 0.38), (0.0, 0.05)]))
    im = frame(W, H, **kw)
    name = f"bar{i:02d}_{W}x{H}.png"
    im.save(os.path.join(OUT, "frames", name))
    bar = R.find_bar(im)
    c = {"file": name, "find_bar": jsonable(bar), "last_fail": R.LAST_FAIL[0]}
    if bar:
        spot = dict(bar); spot["frame"] = list(bar["frame"])
        c["bar_at"] = jsonable(R.bar_at(im, [spot]))
        c["fill"] = list(R.measure_fill(im, bar))
        c["fill_any"] = jsonable(R.measure_fill_any(im, bar))
        c["break"] = list(R.measure_break(im, bar))
        c["verify"] = R.verify_bar(im, bar)
        c["bossrush_bar"] = jsonable(R.as_bossrush_bar(bar)); c["world_bar"] = jsonable(R.as_world_bar(bar))
        scale = bar["span"] / R.REF_SPAN
        tb, xb = R.box_at(bar, R.TITLE_BOX), R.box_at(bar, R.XN_BOX)
        c["boxes"] = [list(tb), list(xb), list(R.timer_box(W, H)), list(R.timer_box(W, H, True))]
        c["prep_text"] = digest(R.prep_text(im, tb, scale))
        c["has_red"] = R.has_red(im, tb, scale)
        c["prep_red"] = digest(R.prep_red(im, tb, scale))
        c["prep_short"] = digest(R.prep_short(im, xb, scale))
        c["prep_white"] = digest(R.prep_white(im, xb, scale))
        c["prep_grey"] = digest(R.prep_grey(im, tb, scale))
        c["prep_bright"] = digest(R.prep_bright(im, tb, scale))
        c["grey_crop"] = digest(R.grey_crop(im, tb))
        c["ink_aspect"] = R.ink_aspect(im, xb)
        sig = R.name_signature(im.crop(tuple(int(v) for v in tb)))
        c["name_signature"] = [list(sig[0]), hashlib.sha256(sig[1]).hexdigest()[:20]]
        crop = im.crop(tuple(int(v) for v in xb))
        c["xn_mask"] = hashlib.sha256(X.mask_bytes(crop)[2]).hexdigest()[:20]
        c["xn_glyphs"] = jsonable(X.split_touching(X.white_mask(crop), X.glyphs(X.white_mask(crop))))
        c["xn_read"] = X.read_counter(crop, TPL, top=21)
        c["prep_pill"] = digest(R.prep_pill(im, tb, scale)) if R.prep_pill(im, tb, scale) else None
    cases.append(c)

texts = ["Lv.60 Young Omega Sherro", "Lv•65 Omega Infergon", "LV 064 Alpha Panpanta", "Lv. 551 Omega X", "Lv.O7 Tucking",
         "| Lv.80 Sparkling- Young Omega Sherro |", "Lv.81 Too High", ";parkling- Omega Tucking", "Lv 6 arkdjng- Alpha Glameep",
         "no level here", "", None, "Ly.5O  Big   Boss  l", "Lv12Glameep", "Tfkling-Omega Bouldus", "økl}ing- Omega Minespine",
         "Sparkling Omega Bouldus", "Tucking Omega Bouldus", "Lv.0 Zero", "LN, 33 Omega"]
xns = ["x21 / x21 / x21", "x5 x5 x7", "x77", "x70 x70", "x17", "xl6 x16 xI6", "× 3", "x123", "x2l", "X9 x9 x10 x10",
       "xS xO", "x||", "x9 x12 x12 x9", "nothing", "", None, "x7x7x1", "x 22 x22", "x22", "x0"]
timers = ["4:59", "0:07", "12.30", "1:5", "123:45", "9 : 08", "x 1:23 4", "07:99", "1:234", "", None, "a12:34b", "1.00 2:30"]
for _ in range(40):
    xns.append(" ".join(rnd.choice(["x", "X", "×", "x "]) + "".join(rnd.choice("0123456789lIi|OoDQSs7") for _ in range(rnd.randint(1, 3))) for _ in range(3)))
    timers.append("".join(rnd.choice("0123456789:. ab") for _ in range(rnd.randint(2, 8))))
pt = {"title": [[t, list(R.parse_title(t))] for t in texts],
      "pair": [[a, b, list(R.parse_title_pair(a, b))] for a, b in [("Omega X", "Lv.60 x"), ("Lv.5 X", "Lv.70"), ("X", None), ("X", "")]],
      "xn": [[t, top, R.parse_xn(t, top)] for t in xns for top in (21, 14)],
      "timer": [[t, R.parse_timer(t)] for t in timers],
      "norm": [[t, R.norm(t)] for t in texts],
      "after_label": [[t, l, R.after_label(t, l)] for t in ["Player name: 0", "Player name: / 0", "Defeats: 12", "", None, "x"] for l in ["Player name:", "Defeats:"]],
      "damage": [[m, f, x, R.damage_dealt(m, f, x, [0, 0.1, 0.3, 0.6, 1.0], br)] for m in (1000.0, 333333.0) for f in (0.0, 0.42, 1.0) for x in (None, 1, 2, 4, 5) for br in (True, False)]}
with open(os.path.join(OUT, "reader_goldens.json"), "w", encoding="utf-8", newline="\n") as f:
    json.dump({"frames": cases, "parse": pt}, f)
print(len(cases), "frames,", sum(1 for c in cases if c["find_bar"]), "with a bar")
