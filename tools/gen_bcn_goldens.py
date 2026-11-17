"""Goldens for alogs-pil's bcn decoder (crates/alogs-pil/tests/bcn.rs): Pillow's own "bcn" decoder on made-up blocks.

The blocks come from a xorshift32 stream both sides generate; for BC7 the first byte is forced to each mode in turn
(and some all-zero blocks), so every mode is covered.
    python tools/gen_bcn_goldens.py   (writes crates/alogs-pil/tests/data/bcn_goldens.json)
"""
import hashlib, json, os
from PIL import Image

def stream(seed):
    x = seed
    while True:
        x ^= (x << 13) & 0xffffffff; x ^= x >> 17; x ^= (x << 5) & 0xffffffff
        yield x & 0xff

def blocks(n, seed, w, h):
    size = 8 if n in (1, 4) else 16
    g = stream(seed)
    data = bytearray(next(g) for _ in range((w // 4) * (h // 4) * size))
    if n == 7:
        for i in range(0, len(data), size):
            k = i // size
            data[i] = 0 if k % 9 == 8 else ((data[i] | 1) << (k % 9)) & 0xff     # mode k % 9 (8: none set)
    return bytes(data)

out = []
for n, mode in ((1, 'RGBA'), (3, 'RGBA'), (4, 'L'), (7, 'RGBA')):
    for seed, (w, h) in ((1, (16, 16)), (7, (64, 32)), (99, (8, 128))):
        im = Image.frombytes(mode, (w, h), blocks(n, seed, w, h), 'bcn', n)
        out.append({'n': n, 'seed': seed, 'size': [w, h], 'md5': hashlib.md5(im.tobytes()).hexdigest()})
path = os.path.join(os.path.dirname(__file__), '..', 'crates', 'alogs-pil', 'tests', 'data', 'bcn_goldens.json')
with open(path, 'w') as f: json.dump(out, f, indent=1)
print(len(out), 'cases')
