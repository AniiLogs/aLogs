"""The Special Offering's circle, looked at twice a second; your own team's blanks, filled from the fight.

    python tests/party_circle.py

Meter.party_circle (8 Nov: a changed offering showed only at the next whole read, 3 s later):
- a new picture is read once it holds still, and replaces what was there;
- a picture still moving isn't read, nor one that's already been read;
- a changed circle that doesn't read goes blank: the old offering isn't kept;
- the screen fading into the fight (no Season Buff at rest) keeps the last read;
- a whole read doesn't bring back an older offering.
Meter.team_tile: a rejected read isn't kept, so the same picture is matched again (a tile stayed blank for 50 s).
Meter.read_fight_team / fight_team:
- a blank slot of yours is filled once two frames name the same Aniimo; one frame isn't enough;
- a read that isn't clear, your lead, or an Aniimo already in the team doesn't count; a slot already read stays;
- with no blank slot nothing is looked at.
Made-up pictures; the matchers are stubbed. Exit code 1 on any failure.
"""
import os, sys, tempfile, logging
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_circle_')
from PIL import Image, ImageDraw
from live import Meter
logging.disable(logging.CRITICAL)

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

W, H = 3840, 2160
OFFER = {(200, 0, 0): 'Hourglass', (0, 200, 0): 'Fan'}
reads = []
class Icons:
    fade = None
    buffs = {'3': {'name': 'Hourglass', 'kind': 'offering', 'icon': 'Hourglass'}, '4': {'name': 'Fan', 'kind': 'offering', 'icon': 'Fan'},
             '5': {'name': 'Guard', 'kind': 'season', 'icon': 's'}}
    def digit(self, im, box): return None
    def buffs_named(self, icon, kind, level=None): return [k for k, b in self.buffs.items() if b['icon'] == icon]
    def buff(self, im, box, kind, text_below=True):
        colour = im.getpixel((int((box[0] + box[2]) / 2), int((box[1] + box[3]) / 2)))[:3]
        if kind == 'season': return ('s', 0.9, 0.4) if colour == (255, 255, 255) else ('s', 0.2, 0.01)
        reads.append(colour)
        return (OFFER[colour], 0.9, 0.4) if colour in OFFER else ('x', 0.3, 0.05)

def screen(colour, rest=True):
    im = Image.new('RGB', (W, H), (40, 40, 60)); d = ImageDraw.Draw(im); x0, y0, x1, y1 = Meter.OFFERING
    d.rectangle((W * x0, H * y0, W * x1, H * y1), fill=colour)
    if rest:
        x0, y0, x1, y1 = Meter.SEASON_BUFFS[0]; d.rectangle((W * x0, H * y0, W * x1, H * y1), fill=(255, 255, 255))
    return im
def meter():
    m = Meter.__new__(Meter); m.icons = Icons(); m.icons_state = 'ready'
    m.party = {'at': 0, 'members': [{'slot': 0, 'name': 'Alpha', 'self': True, 'ai': False, 'pill': 0, 'aniimo': 'Leader'}], 'votes': {}, 'counted': set()}
    m.clock = 100.0
    return m
def show(m, im, times=2):
    for _ in range(times):
        m.clock += 0.5; m.party_circle(im, m.clock, W, 0, H)
        if m.circle_job: m.circle_job.join()
def offering(m): return ((m.party.get('buff_reads') or {}).get('offering') or {}).get('name')

m = meter()
show(m, screen((200, 0, 0)), 1); check('one look: still might be moving', offering(m), None)
show(m, screen((200, 0, 0)), 1); check('held still: read', offering(m), 'Hourglass')
n = len(reads); show(m, screen((200, 0, 0)), 6); check('the same picture is not read again', len(reads), n)
show(m, screen((0, 200, 0)), 2); check('changed: the new one replaces it', offering(m), 'Fan')
m.party['buff_reads']['season'] = [{'name': 'Guard'}, None]
show(m, screen((90, 90, 90)), 2); check('changed and unreadable: blank, not the old one', offering(m), None)
check('...the Season Buffs stay', m.party['buff_reads']['season'][0], {'name': 'Guard'})
check('...and the run has no offering', [b['name'] for b in m.party['buffs']], ['Guard'])
show(m, screen((0, 200, 0)), 2); check('readable again', offering(m), 'Fan')
show(m, screen((10, 10, 10), rest=False), 3); check('fading into the fight: the last read stands', offering(m), 'Fan')

m = meter(); show(m, screen((90, 90, 90)), 2); n = len(reads)
show(m, screen((90, 90, 90)), 2); check("a picture that doesn't read isn't tried on every look", len(reads), n)
show(m, screen((90, 90, 90)), 8); check('...but is tried again in a while', len(reads) > n, True)

m = meter(); show(m, screen((0, 200, 0)), 2); m.team_tile = lambda *a, **k: None; m.card_shift = lambda *a, **k: (0, 0)
m.party['buff_reads']['offering'] = {'name': 'Fan'}
m.read_party_job(screen((200, 0, 0)), [(0, 0)], W, 0, H, {0: m.who(m.party['members'][0])})
check("a whole read's older frame doesn't change it", offering(m), 'Fan')

# -- a rejected tile is matched again ---------------------------------------------------------------------------------
class Tile:
    fade = None; calls = 0; answer = ('Budling', 0.7, 0.05, 'k')
    def badge(self, *a): return (0, 0, 0)
    def head(self, im, window, square, fade=False): Tile.calls += 1; return Tile.answer
m = Meter.__new__(Meter); a, b = Meter.PARTY_CARDS[0][:2]; im = screen((90, 90, 90))
check('not clear enough', Meter.tile_sure(m.team_tile(Tile(), im, a, b, W, 0, H, memo=(0, 0))), False)
m.team_tile(Tile(), im, a, b, W, 0, H, memo=(0, 0)); check('the same picture is matched again', Tile.calls, 2)
Tile.answer = ('Budling', 0.7, 0.2, 'k')
check('its next read counts', Meter.tile_sure(m.team_tile(Tile(), im, a, b, W, 0, H, memo=(0, 0))), True)
m.team_tile(Tile(), im, a, b, W, 0, H, memo=(0, 0)); check('...and is kept', Tile.calls, 3)

# -- your team from the fight -------------------------------------------------------------------------------------------
class Hud:
    def __init__(self, rows): self.rows = rows; self.asked = []
    def head(self, im, window, square, fade=False):
        row = min(range(3), key=lambda i: abs(square[1] + square[2] / 2 - H * Meter.HUD_TEAM[1][i]))
        self.asked.append(row); return self.rows[row]
def fight_meter(team, rows):
    m = meter(); m.icons = Hud(rows); m.party['teams'] = [team, None, None, None]; m.party['team_who'] = {}
    return m
clear = lambda name: (name, 0.8, 0.3, 'k')
def look(m): m.read_fight_team(screen((1, 1, 1)), m.party, m.party['members'][0], W, 0, H)
m = fight_meter([None, 'Bravo', 'Cinder'], [clear('Dune'), clear('Bravo'), clear('Cinder')])
look(m); check('one frame is not enough', m.party['teams'][0], [None, 'Bravo', 'Cinder'])
look(m); check('two agreeing fill the blank', m.party['teams'][0], ['Dune', 'Bravo', 'Cinder'])
check('...and it goes with the run', m.party['team'], ['Dune', 'Bravo', 'Cinder'])
check('slots already read are not looked at', set(m.icons.asked), {0})
m = fight_meter(None, [clear('Dune'), ('Ember', 0.5, 0.05, 'k'), None])
look(m); look(m); check('no read at all: only the clear one', m.party['teams'][0], ['Dune', None, None])
m = fight_meter([None, 'Bravo', None], [clear('Leader'), None, clear('Bravo')])
look(m); look(m); check('your lead, or one already in the team, is not taken', m.party['teams'][0], [None, 'Bravo', None])
m = fight_meter([None, None, None], [clear('Dune'), None, None]); look(m); m.icons.rows = [clear('Ember'), None, None]; look(m)
check('two different names in two frames: not filled', m.party['teams'][0], [None, None, None])
m = fight_meter(['Dune', 'Bravo', 'Cinder'], [None, None, None])
m.fight_team(screen((1, 1, 1)), 500.0, W, 0, H); check('no blank: no look', m.hud_job, None)
m = fight_meter([None, 'Bravo', 'Cinder'], [clear('Dune'), None, None])
m.fight_team(screen((1, 1, 1)), 500.0, W, 0, H); m.hud_job.join(); first = len(m.icons.asked)
m.fight_team(screen((1, 1, 1)), 501.0, W, 0, H); m.hud_job.join(); check('not on every frame', len(m.icons.asked), first)

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('party circle: ok')
