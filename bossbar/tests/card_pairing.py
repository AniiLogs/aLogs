"""Each result-screen player gets the Battle Data card of its own slot, in the run sent and in your bests.

    python tests/card_pairing.py

A slot the result screen didn't read (no name and no value) drops out of its player list; paired by position, every
player after it took the next slot's card (1 Nov: your row went with another player's Avg DPS, BREAK, damage taken and
heal). That slot's card goes too, as its player (5 sent runs left one out). Made-up players and numbers; the data folder is a temporary one. Exit code 1 on any failure.
"""
import os, shutil, sys, tempfile, types
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
TMP = tempfile.mkdtemp(prefix='alogs_cards_')
os.environ['ALOGS_DATA'] = TMP
import live, submit as S

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

def card(slot, dps): return {'slot': slot, 'name': f'P{slot + 1}', 'avg_dps': dps, 'break': dps // 10, 'taken': dps // 20, 'heal': slot}
CARDS = [card(0, 10000), card(1, 20000), card(2, 30000), card(3, 40000)]
def player(slot, me=False): return {'slot': slot, 'name': f'P{slot + 1}', 'stat': 'Damage', 'value': slot + 1, 'self': me}

def summary(players, cards):
    return {'result': {'players': players, 'score': 30000, 'rating_stars': 30, 'time_s': 150, 'cleared': False},
            'battle_data': {'players': cards}, 'meter': {'csv': 'run.csv'}, 'cycle': 4001, 'puppetId': 1}

# slot 2 (3P) not read on the result screen: 4P keeps 4P's card, not a shifted one, and 3P goes as its card's
# player (the card's name, no stat or value), in its place, counted in the party
sent = S.run_payload(summary([player(0), player(1), player(3, me=True)], CARDS), 'uid')
check('dropped slot: Avg DPS by player', [p.get('avgDps') for p in sent['players']], [10000, 20000, 30000, 40000])
check('dropped slot: heal by player', [p.get('heal') for p in sent['players']], [0, 1, 2, 3])
check('dropped slot: its card player', sent['players'][2], {'name': 'P3', 'avgDps': 30000, 'breakValue': 3000, 'damageTaken': 1500, 'heal': 2})
check('dropped slot: party size', sent['partySize'], 4)
check('dropped slot: you once', [p.get('self') for p in sent['players']], [None, None, None, True])
# a slot kept for its stat label alone (no name, no value): its card's name, the label, no value
sent = S.run_payload(summary([player(0), {'slot': 1, 'name': '', 'stat': 'Most Healing', 'value': None}, player(2), player(3)], CARDS), 'uid')
check('label only: sent', sent['players'][1], {'name': 'P2', 'stat': 'Most Healing', 'avgDps': 20000, 'breakValue': 2000, 'damageTaken': 1000, 'heal': 1})
# a solo run: one card, one player, nothing added
sent = S.run_payload(summary([player(0, me=True)], CARDS[:1]), 'uid')
check('solo: one player', [p.get('avgDps') for p in sent['players']], [10000])
# every slot read: as before
sent = S.run_payload(summary([player(i) for i in range(4)], CARDS), 'uid')
check('all slots: Avg DPS', [p.get('avgDps') for p in sent['players']], [10000, 20000, 30000, 40000])
# one card fewer than players (a card not counted on screen): the last player has none, the others theirs
sent = S.run_payload(summary([player(i) for i in range(4)], CARDS[:3]), 'uid')
check('missing card: Avg DPS', [p.get('avgDps') for p in sent['players']], [10000, 20000, 30000, None])
# no result screen read: the cards alone, each its own player
sent = S.run_payload(summary([], CARDS[:2]), 'uid')
check('no result players: cards', [p.get('avgDps') for p in sent['players']], [10000, 20000])
# summaries saved before slots were kept: by position, as they were sent then
old = [{k: v for k, v in p.items() if k != 'slot'} for p in (player(0), player(1))]
sent = S.run_payload(summary(old, [{k: v for k, v in c.items() if k != 'slot'} for c in CARDS[:2]]), 'uid')
check('no slots: by position', [p.get('avgDps') for p in sent['players']], [10000, 20000])

# your bests: your own card, not the one at your place in the list
m = types.SimpleNamespace(uid='u1'); os.makedirs(os.path.join(live.HERE, 'logs'), exist_ok=True)
got = live.Meter.beats_best(m, summary([player(0), player(1), player(3, me=True)], CARDS))
best = __import__('json').load(open(os.path.join(live.HERE, 'logs', 'bests.json'), encoding='utf-8'))
check('bests: your Avg DPS', next(iter(best.values())).get('dps'), 40000)
check('bests: your heal', next(iter(best.values())).get('heal'), 3)
# a player with no name and no value in the list (kept for its stat label): your card is still yours
os.remove(os.path.join(live.HERE, 'logs', 'bests.json'))
got = live.Meter.beats_best(m, summary([player(0), {'slot': 1, 'name': '', 'stat': 'Most Healing', 'value': None},
                                         player(2), player(3, me=True)], CARDS))
best = __import__('json').load(open(os.path.join(live.HERE, 'logs', 'bests.json'), encoding='utf-8'))
check('bests with a label-only player: your Avg DPS', next(iter(best.values())).get('dps'), 40000)

shutil.rmtree(TMP, ignore_errors=True)
if fails:
    print(f'{len(fails)} FAILED:'); [print('  ' + f) for f in fails]; sys.exit(1)
print('Battle Data cards: each player gets its own slot\'s card, in the run sent and in your bests')
