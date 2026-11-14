"""One player per slot, a Korean name never sent as a Latin look-alike, and AI only on a sign read twice.

    python tests/party_lineup_guards.py

On 14 Nov one shared run came from two PCs. One read it right; the other sent a teammate's jamo-only Korean name as one
Latin letter, another teammate in two slots of the lineup, and a real player marked AI. Checked here, with made-up names:
- Ocr.read_native: one Latin letter where the name box holds two pieces of ink is left blank; with the game's fonts off
  (KoreanNames: no game install found) a box the Korean model reads as Hangul is left blank, not sent as the Latin
  look-alike (one letter, or a Latin word); a Latin name, and a real one-letter name, read as before. The helper is a
  stand-in that answers as ocr-helper.exe does.
- Meter.set_party: a name read in a second slot (a misread badge) next to the slot kept from a read before leaves the
  weaker slot blank; a card with no CP on one read isn't AI, on two it is, and a CP read once keeps the player human.
- results.party_slots: a slot the Party screen left blank gives its column no name.
- submit.lineup_names: a lineup card's misread of a result-screen name takes that name, paired by likeness even when the
  slots differ, never twice; a sure one-character name takes its slot; a duplicate goes blank; someone else stays.
Exit code 1 on any failure.
"""
import json, os, struct, sys, tempfile, threading
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_guards_')
from PIL import Image, ImageDraw
import live, reader as R, hangulnames as K, results, submit

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

# --- name reads ---------------------------------------------------------------------------------------------------
PAD = 24
def box(pieces):
    """A prepared name box: dark strokes on white. 'jamo': a wide L and a 7 side by side (two pieces); 'one': an L."""
    im = Image.new('L', (260 + 2 * PAD, 70 + 2 * PAD), 255); d = ImageDraw.Draw(im)
    x, y = PAD + 30, PAD + 12
    d.line((x, y, x, y + 44), fill=0, width=7); d.line((x, y + 44, x + 40, y + 44), fill=0, width=7)       # an L / ㄴ
    if pieces == 'jamo':
        d.line((x + 70, y, x + 110, y), fill=0, width=7); d.line((x + 110, y, x + 110, y + 44), fill=0, width=7)   # a ㄱ
    return im

class Helper:
    """Answers OCR2 requests as ocr-helper.exe: per box 'windows text<TAB>Korean letters<TAB>Latin text'."""
    def __init__(self, latin, letters):
        self.latin, self.letters, self.buf, self.out = latin, letters, b'', []
        self.stdin, self.stdout = self, self
    def write(self, data): self.buf += data
    def flush(self):
        data, self.buf = self.buf, b''
        n = struct.unpack('<I', data[4:8])[0]; at = 8
        for _ in range(n):
            flags = data[at]; w, h = struct.unpack('<II', data[at + 1:at + 9]); at += 9 + w * h
            korean = json.dumps(self.letters) if flags & 1 else ''
            self.out.append(f"\t{korean}\t{'' if flags & 1 else self.latin}\n".encode())
    def readline(self): return self.out.pop(0)

def read(img, latin, letters, korean=None):
    o = R.Ocr.__new__(R.Ocr)
    o.native, o.lock, o.cache_lock, o.name_cache, o.gen, o.keep_unsure = True, threading.Lock(), threading.Lock(), {}, 0, True
    o.p = Helper(latin, letters); R.Ocr.korean = korean
    try: return o.read({'name0': img}, names={'name0': PAD})['name0'], o
    finally: R.Ocr.korean = None

hangul = [[40, [['ㄴ', 0.93], ['L', 0.05]]], [110, [['ㄱ', 0.91], ['7', 0.06]]]]
latin = [[40, [['L', 0.9]]], [110, [['7', 0.8]]]]
check('tidied ink: a jamo-only name is two pieces', K.ink_pieces(K.tidy(box('jamo'), PAD)), 2)
check('...one letter is one', K.ink_pieces(K.tidy(box('one'), PAD)), 1)
check('the Korean model reading jamo looks Hangul', K.looks_hangul(hangul), True)
check('...its Latin read does not', K.looks_hangul(latin), False)
check('...one stray jamo in a Latin word does not', K.looks_hangul([[0, [['B', .9]]], [9, [['ㅏ', .9]]], [18, [['x', .9]]]]), False)

got, o = read(box('jamo'), 'L', hangul)
check('fonts off: one Latin letter for a Hangul box goes blank', got, '')
check('...and is not kept for good (unsure)', results.sure_read(o, box('jamo')), False)
check('fonts off: a Latin word for a Hangul box goes blank', read(box('jamo'), 'Olva', hangul)[0], '')
check('fonts off: a Latin name stays', read(box('jamo'), 'Bramblefox', [[i * 9, [[c, .9]]] for i, c in enumerate('Bramblefox')])[0], 'Bramblefox')
check('one Latin letter over two pieces of ink goes blank', read(box('jamo'), 'L', latin)[0], '')
check('a one-letter name with one piece of ink still reads', read(box('one'), 'L', [[40, [['L', 0.6]]]])[0], 'L')

# --- the Party screen ---------------------------------------------------------------------------------------------
def member(slot, name, pill=None, ai=False, me=False):
    return {'slot': slot, 'name': name, 'self': me, 'ai': ai, 'pill': slot if pill is None else pill}
m = live.Meter.__new__(live.Meter)
m.party = None; m.screen_settled = lambda kind, now: None
names = lambda: [(x['slot'], x['name']) for x in m.party['members']]
ai = lambda: [x['ai'] for x in m.party['members']]

m.set_party([member(0, 'Quillon', me=True), member(1, 'Tarnwick'), member(3, 'Pellamy')], 100)
# the next read: Tarnwick's '2P' badge read as '3P', so the pill in place 2 gives slot 3; slot 2 is kept from before
m.set_party([member(0, 'Quillon', me=True), member(2, 'Tarnwick', pill=1), member(3, 'Pellamy')], 103)
check('one name in two slots: the weaker slot is blank', names(), [(0, 'Quillon'), (1, 'Tarnwick'), (2, ''), (3, 'Pellamy')])
check('...and the blank slot gives its result column no name',
      (lambda ps: (results.party_slots(ps, m.party['members']), ps[2]['name'])[1])([{'slot': i, 'name': ''} for i in range(4)]), '')

m.party = None
m.set_party([member(0, 'Quillon', me=True), member(1, 'Tarnwick', ai=True)], 200)     # one read missed Tarnwick's CP
check('no CP on one read: not AI', ai(), [False, False])
m.set_party([member(0, 'Quillon', me=True), member(1, 'Tarnwick', ai=True)], 203)
check('no CP on two reads: AI', ai(), [False, True])
m.party = None
m.set_party([member(0, 'Quillon', me=True), member(1, 'Tarnwick')], 300)              # CP read once: a real player
m.set_party([member(0, 'Quillon', me=True), member(1, 'Tarnwick', ai=True)], 303)
m.set_party([member(0, 'Quillon', me=True), member(1, 'Tarnwick', ai=True)], 306)
check('a CP read once keeps them human', ai(), [False, False])
m.party = None
m.set_party([member(0, 'Quillon', me=True), member(1, 'Flora', ai=True)], 400)
check("one of the game's AI teammates by name: AI at once", ai(), [False, True])

# --- the lineup sent with a run -----------------------------------------------------------------------------------
def lineup(party, players):
    out = submit.lineup_names(party, players)
    return [out[id(x)] for x in party]
rows = [{'slot': 0, 'name': 'Quillon', 'self': True}, {'slot': 1, 'name': 'Bramblefox'},
        {'slot': 2, 'name': 'Wren_ShadeKD'}, {'slot': 3, 'name': '0', 'sure': True}]
party = [member(0, 'Quillon', me=True), member(1, 'Wn_JhaduKD'), member(2, 'Brarnb1efux'), member(3, 'TUN')]
check('lineup misreads take the result names, paired across slots', lineup(party, rows),
      ['Quillon', 'Wren_ShadeKD', 'Bramblefox', '0'])
check('a one-character name not sure of keeps the card read',
      lineup(party, rows[:3] + [{'slot': 3, 'name': '0'}]), ['Quillon', 'Wren_ShadeKD', 'Bramblefox', 'TUN'])
check('a weak likeness sharing a stretch of letters is that player',
      lineup([member(0, 'Quillon', me=True), member(1, 'ZzPnyhru')], [rows[0], {'slot': 1, 'name': 'Mytharu'}]), ['Quillon', 'Mytharu'])
check('...one with letters only here and there is someone else',
      lineup([member(0, 'Quillon', me=True), member(1, 'Zarvok')], [rows[0], {'slot': 1, 'name': 'Ashgrove'}]), ['Quillon', 'Zarvok'])
check('someone else stays as read', lineup([member(0, 'Quillon', me=True), member(1, 'Haverlock')], rows[:2]),
      ['Quillon', 'Haverlock'])
check('a duplicate in the lineup goes blank in the weaker slot',
      lineup([member(0, 'Quillon', me=True), member(1, 'Tarnwick'), member(2, 'Tarnwick', pill=1)],
             [{'slot': 0, 'name': 'Quillon', 'self': True}, {'slot': 1, 'name': 'Tarnwick'}]), ['Quillon', 'Tarnwick', ''])
check('...also with no result screen', lineup([member(1, 'Tarnwick'), member(2, 'Tarnwick', pill=1)], []), ['Tarnwick', ''])

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('party lineup guards: ok')
