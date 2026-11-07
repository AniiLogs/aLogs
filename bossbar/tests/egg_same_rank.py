"""An Egg Heist standing the site already took from this app isn't sent again, even after a restart.

    python tests/egg_same_rank.py

submit.standing_sent / remember_standing: per UID and season, the last standing sent (tier, subtier, rank points,
stars). The same again -> not sent; any change, another season or another UID -> sent. Exit code 1 on any failure.
"""
import os, sys, tempfile
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_egg_')
import submit
submit.STANDINGS = os.path.join(tempfile.mkdtemp(prefix='alogs_egg_'), 'sent_standings.json')

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

st = {'tier': 3, 'subtier': 2, 'name': 'Gold II', 'points': 1337, 'stars': None}
p = lambda **c: submit.eggheist_payload({**st, **c}, 'S2', '302000000001', {'nextRankAt': 1350})
check('nothing sent yet', submit.standing_sent(p()), False)
submit.remember_standing(p())
check('the same standing', submit.standing_sent(p()), True)
check('more rank points', submit.standing_sent(p(points=1355)), False)
check('a new subtier', submit.standing_sent(p(subtier=3)), False)
check('another season', submit.standing_sent(submit.eggheist_payload(st, 'S3', '302000000001')), False)
check('another UID', submit.standing_sent(submit.eggheist_payload(st, 'S2', '302000000002')), False)
submit.remember_standing(p(points=1355))
check('the newer one is remembered', (submit.standing_sent(p(points=1355)), submit.standing_sent(p())), (True, False))

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('egg same rank: ok')
