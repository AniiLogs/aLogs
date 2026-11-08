"""A screen's title is matched on its letters: OCR's stray space doesn't lose the screen.

    python tests/screen_titles.py

Meter.title_is / screen_spotted (8 Nov: at 4K one Party screen's title read 'p arty' on every read but one for 35 s, so
it was spotted once and never read: a run went in with no teams and no buffs):
- 'p arty', 'Party', 'Party Confir' and 'P A R T Y' are the Party screen; so are 'p rofile' and 'Profile' the Profile;
- 'Squad', 'Battle Buffs', an empty title and a word that only holds the letters later on aren't.
Meter.party_up: once the title has been read, 'Environment info' keeps the Party screen up until the fight starts, so
it goes on being read (8 Nov: read once in 26 s, 20 s before the fight); a Squad screen with the same text doesn't.
Exit code 1 on any failure.
"""
import os, sys, tempfile
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_titles_')
from live import Meter

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

for title in ('p arty', 'Party', 'party', ' Party ', 'Party Confir', 'Party Co', 'P A R T Y', 'p-arty'):
    check(f'{title!r} is the Party screen', Meter.title_is(title, 'party'), True)
    check(f'{title!r} spotted', Meter.screen_spotted({'h': title}), 'party')
for title in ('', None, 'Squad', 'S uad', 'Battle Buffs', 'Holo-Battle I', 'Counterparty', 'par'):
    check(f'{title!r} is not the Party screen', Meter.title_is(title, 'party'), False)
    check(f'{title!r} not spotted', Meter.screen_spotted({'h': title or ''}) == 'party', False)
for title in ('Profile', 'p rofile', 'Pro file'):
    check(f'{title!r} is the Profile screen', Meter.title_is(title, 'profile'), True)
    check(f'{title!r} spotted', Meter.screen_spotted({'h': title}), 'profile')

# -- the Party screen stays up while its title is hard to read (Meter.party_up, read_screen) --------------------------
from PIL import Image
def meter():
    m = Meter.__new__(Meter); m.fight = None; m.party_circle = lambda *a: None; return m
ENV = 'Environment info'
m = meter()
check('no title yet: the text alone is not a Party screen', m.party_up({'h': '', 'bs': ENV}, 100), False)
check('a title', m.party_up({'h': 'p arty', 'bs': ENV}, 101), True)
check('then no title, the text', m.party_up({'h': '', 'bs': ENV}, 103), True)
check('a garbled title', m.party_up({'h': 'Parfy', 'bs': 'RS Envjronment info'}, 105), True)
check('a Squad title', m.party_up({'h': 'Squad', 'bs': ENV}, 107), False)
check('Battle Buffs over it', m.party_up({'h': 'Battle Buffs'}, 109), False)
check('back: still up', m.party_up({'h': '', 'bs': ENV}, 111), True)
check('no text and no title', m.party_up({'h': '', 'bs': ''}, 113), False)
check('long after its last sign', m.party_up({'h': '', 'bs': ENV}, 111 + Meter.PARTY_KEEP_S + 1), False)
m = meter(); m.party_up({'h': 'Party'}, 100); m.fight = object()
check('a fight ends it', m.party_up({'h': 'Party', 'bs': ENV}, 101), False)
m.fight = None
check('and afterwards the text alone does not bring it back', m.party_up({'h': '', 'bs': ENV}, 102), False)

# read_screen acts on it: the reader starts on a whole frame; a frame of only the boxes asks for whole frames first
import types
m = meter(); calls = []
m.args = types.SimpleNamespace(replay=None, no_auto_click=False); m.catch_seen = 0
m.screens_gone = lambda now: None; m.menu_name = lambda *a: None; m.profile_notes = lambda *a: None; m.auto_active = lambda: False
m.test_frame = lambda *a: None; m.party_names = lambda *a: {}; m.names_first = lambda handler, *a, **k: calls.append(handler.__name__)
whole = Image.new('RGB', (64, 36)); parts = Image.new('RGB', (64, 36)); parts.info['parts'] = True
m.read_screen(whole, 100, {'h': 'Party', 'bs': ENV})
check('the title starts the read', calls, ['party_screen'])
m.party_seen = 100; calls.clear()
m.read_screen(whole, 103, {'h': '', 'bs': ENV})
check('no title, whole frame: read again', calls, ['party_screen'])
calls.clear(); m.full_until = 0
m.read_screen(parts, 106, {'h': '', 'bs': ENV})
check('no title, only the boxes: whole frames first', (calls, m.full_until > 106), ([], True))

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('screen titles: ok')
