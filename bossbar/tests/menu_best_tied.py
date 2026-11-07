"""A menu best (battle prep) isn't sent when the site's board already has that score for the player's boss and week.

    python tests/menu_best_tied.py

Submitter.board_has reads the board (/leaderboard/interlink?cycle=) and finds the player by UID: their stars and
score there at least the menu best's -> not sent. A higher menu best, a player not on the board, or a board that
can't be read -> sent as before. Stand-ins only (urlopen is replaced). Exit code 1 on any failure.
"""
import io, json, os, sys, tempfile
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_menu_')
import submit

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

board = {'bosses': [{'id': '80000109', 'entries': [{'uid': '302000000001', 'stars': 20, 'score': 25000}]},
                    {'id': '80000114', 'entries': []}]}
asked = []
class Reply(io.BytesIO):
    status = 200
    def __enter__(self): return self
    def __exit__(self, *a): return False
def urlopen(req, timeout=None):
    asked.append(req.full_url)
    if board is None: raise OSError('offline')
    return Reply(json.dumps(board).encode())
submit.urllib.request.urlopen = urlopen

s = submit.Submitter.__new__(submit.Submitter)
s.account = {'api_url': 'https://api.example/api', 'token': 't'}
best = lambda score, stars=20, boss='80000109', uid='302000000001': {'kind': 'interlink_best', 'cycle': '3005', 'gameUid': uid,
                                                                     'boss': {'id': boss, 'name': 'X'}, 'score': score, 'stars': stars}
check('a tie: not sent', s.board_has(best(25000)), True)
check('lower: not sent', s.board_has(best(24000)), True)
check('higher score: sent', s.board_has(best(26000)), False)
check('more stars: sent', s.board_has(best(20000, stars=21)), False)
check('no entry for that boss: sent', s.board_has(best(1000, boss='80000114')), False)
check('another UID: sent', s.board_has(best(25000, uid='302000000009')), False)
check('the week asked for', asked[0], 'https://api.example/api/leaderboard/interlink?cycle=3005&unlinked=1')
board = None
check("board can't be read: sent", s.board_has(best(25000)), False)

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('menu best tied: ok')
