"""A screen's title is matched on its letters: OCR's stray space doesn't lose the screen.

    python tests/screen_titles.py

Meter.title_is / screen_spotted (8 Nov: at 4K one Party screen's title read 'p arty' on every read but one for 35 s, so
it was spotted once and never read: a run went in with no teams and no buffs):
- 'p arty', 'Party', 'Party Confir' and 'P A R T Y' are the Party screen; so are 'p rofile' and 'Profile' the Profile;
- 'Squad', 'Battle Buffs', an empty title and a word that only holds the letters later on aren't.
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

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('screen titles: ok')
