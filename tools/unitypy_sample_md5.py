"""UnityPy's pictures of crates/alogs-unity/tests/bundle.rs's made-up sample bundle, for that test's WANT table.
    ALOGS_SAMPLE_BUNDLE=<path> cargo test -p alogs-unity --test bundle    (writes the sample)
    python tools/unitypy_sample_md5.py <path>
The sample's type trees hold only the fields aLogs reads, so UnityPy doesn't make its classes of them: this reads
them with read_typetree() and runs UnityPy's own picture code on the fields (parse_image_data; for the sprite the
crop, SpriteHelper.mask_sprite and the flip of get_image_from_sprite).
"""
import hashlib, struct, sys, types
import UnityPy
from PIL import Image
from UnityPy.export import SpriteHelper
from UnityPy.export.Texture2DConverter import parse_image_data

env = UnityPy.load(sys.argv[1])
trees = {o.path_id: o.read_typetree() for o in env.objects}
res = next(f for f in env.files.values()).files['CAB-test.resS']

def texture(t, flip):
    data = t['image data']
    if not data:
        sd = t['m_StreamData']; res.Position = sd['offset']; data = res.read_bytes(sd['size'])
    return parse_image_data(bytes(data), t['m_Width'], t['m_Height'], t['m_TextureFormat'], (2022, 3, 62, 1), 19, None, flip)

def out(name, im):
    print(f'("{name}", ({im.width}, {im.height}), "{im.mode}", "{hashlib.md5(im.tobytes()).hexdigest()}"),')

for pid, t in trees.items():
    if 'm_RD' not in t:
        out(t['m_Name'], texture(t, True)); continue
    rd = t['m_RD']; r = rd['textureRect']
    img = texture(trees[rd['texture']['m_PathID']], False)
    img = img.crop((r['x'], r['y'], r['x'] + r['width'], r['y'] + r['height']))
    vd = rd['m_VertexData']; n = vd['m_VertexCount']
    verts = [struct.unpack_from('<3f', bytes(vd['m_DataSize']), 12 * k) for k in range(n)]
    idx = struct.unpack(f'<{len(rd["m_IndexBuffer"]) // 2}H', bytes(rd['m_IndexBuffer']))
    mesh = types.SimpleNamespace(m_Vertices=verts, get_triangles=lambda: [[idx[k:k + 3] for k in range(0, len(idx), 3)]])
    img = SpriteHelper.mask_sprite(types.SimpleNamespace(m_PixelsToUnits=t['m_PixelsToUnits']), mesh, img)
    out(t['m_Name'], img.transpose(Image.Transpose.FLIP_TOP_BOTTOM))
