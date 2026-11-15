"""The last check before a record is sent (submit.send_check): no player or lineup slot named twice, and no Aniimo species
that isn't one from the game's list (14 Nov: the owner wants misreads stopped in the meter, never repaired on the site).

    python tests/send_check.py

Made-up records only. Exit code 1 on any failure.
"""
import copy, os, sys
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__))); sys.path.insert(0, HERE)
import logging; logging.disable(logging.CRITICAL)
import submit as S

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

SPECIES = {'irisalis', 'somniwing', 'leafy', 'pawney', 'turbo', 'tromber', 'stellarys'}
run = {'kind': 'interlink_run', 'clientRunId': 'r1',
       'players': [{'name': 'Alpha', 'avgDps': 5}, {'name': 'Bravo', 'self': True, 'avgDps': 6},
                   {'name': 'Bravo', 'avgDps': 7}, {'name': 'AIhelper', 'ai': True}, {'name': 'AIhelper', 'ai': True}],
       'extra': {'lineup': [
           {'slot': 0, 'name': 'Alpha', 'aniimo': 'Irisalis', 'team': [{'aniimo': 'Irisalis'}, {'aniimo': 'Somniwing'}]},
           {'slot': 1, 'name': 'Bravo', 'self': True, 'aniimo': 'Tiamat', 'team': [{'aniimo': 'Tiamat'}, {'aniimo': 'Leafy'}]},
           {'slot': 2, 'name': 'A1pha', 'aniimo': 'otellarys', 'team': [{'aniimo': 'otellarys'}]},
       ]}}
before = copy.deepcopy(run)
out = S.send_check(run, SPECIES)
check('the input is left as it was', run, before)
check('Battle Data: the second Bravo goes unnamed, the one marked you keeps it',
      [p.get('name') for p in out['players']], ['Alpha', 'Bravo', None, 'AIhelper', 'AIhelper'])
check('a numbers row keeps its numbers when unnamed', out['players'][2]['avgDps'], 7)
check('lineup: a look-alike of a name already sent (A1pha / Alpha) goes unnamed', [e.get('name') for e in out['extra']['lineup']], ['Alpha', 'Bravo', None])
check('a real lead stays', out['extra']['lineup'][0].get('aniimo'), 'Irisalis')
check("a lead that isn't an Aniimo is kept only as its nickname", (out['extra']['lineup'][1].get('aniimo'), out['extra']['lineup'][1].get('nickname')), (None, 'Tiamat'))
check('...and its team keeps only Aniimo', out['extra']['lineup'][1]['team'], [{'aniimo': 'Leafy'}])
check("a cut-off read isn't a species; a team with nothing left goes", ('aniimo' in out['extra']['lineup'][2], 'team' in out['extra']['lineup'][2]), (False, False))
check('a nickname already read is kept', S.send_check({'extra': {'lineup': [{'name': 'X', 'aniimo': 'Tiamat', 'nickname': 'Own'}]}}, SPECIES)['extra']['lineup'][0]['nickname'], 'Own')

sim = {'kind': 'sim_run', 'clientRunId': 's1', 'extra': {'setup': {'team': [
    {'name': 'Irisalis', 'level': 70, 'mvp': True, 'nickname': 'Sparkles'}, {'name': 'Pawne', 'level': 60}, {'name': 'Repair Matrix'}, {'name': 'Turbo'}]}}}
check('Sim team: only Aniimo go out', [m['name'] for m in S.send_check(sim, SPECIES)['extra']['setup']['team']], ['Irisalis', 'Turbo'])

none = S.send_check(run, None)
check("the game's list unreadable: species left as read (never guessed)", none['extra']['lineup'][1].get('aniimo'), 'Tiamat')
check('...names still checked', [p.get('name') for p in none['players']][:3], ['Alpha', 'Bravo', None])
check('a record with none of these passes untouched', S.send_check({'kind': 'egg', 'score': 3}, SPECIES), {'kind': 'egg', 'score': 3})

if fails:
    print('FAIL\n  ' + '\n  '.join(fails)); sys.exit(1)
print('PASS')
