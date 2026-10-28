"""Egg Heist season rank: the rank ladder and reading the game's "Rank Info" screen.

The ladder comes from the game data (egg_rank_base_data) and the formula the game's own Rank Info screen uses;
it reproduces the screen exactly (e.g. 1277 points = Pro Egg Snatcher III, "1277/2370"). See
aniimo_data/egg_heist_rank.md. What gets recorded and submitted is what the screen shows: rank name, Rank Points
and, at Egg Emperor, stars - the ladder only fills in the sub-tier (IV, III, ...) the title doesn't spell out.
"""
import re

# (big rank name, sub-tier labels from lowest to highest, eggs per sub-tier, points per egg)
RANKS = [
    ('Rookie Egg Hunter', ['III', 'II', 'I'], 3, 30),
    ('Rising Egg Scout', ['III', 'II', 'I'], 3, 60),
    ('Pro Egg Snatcher', ['IV', 'III', 'II', 'I'], 4, 90),
    ('Elite Egg Seeker', ['IV', 'III', 'II', 'I'], 5, 120),
    ('Expert Egg Raider', ['V', 'IV', 'III', 'II', 'I'], 6, 150),
    ('Egg Heist Master', ['V', 'IV', 'III', 'II', 'I'], 6, 180),
    ('Egg Emperor', [''], 0, 180),
]
TOP_TIER = len(RANKS)

def ladder():
    """[(tier 1-7, sub-tier index 1.., full name, points where it starts)] in order."""
    tiers = [(bi + 1, si + 1, f'{name} {label}'.strip(), eggs, pt)
             for bi, (name, labels, eggs, pt) in enumerate(RANKS) for si, label in enumerate(labels)]
    out, acc = [], 0
    for i, (tier, sub, name, eggs, pt) in enumerate(tiers):
        out.append((tier, sub, name, acc))
        if i + 1 < len(tiers):
            acc += max(0, eggs - (0 if i == 0 else 1)) * pt + tiers[i + 1][4]
    return out

LADDER = ladder()

def tier_of(title):
    """Big rank (1-7) from the screen title, tolerant of small OCR slips."""
    t = re.sub(r'[^a-z]', '', (title or '').lower())
    for i, (name, *_rest) in enumerate(RANKS):
        if re.sub(r'[^a-z]', '', name.lower()) in t: return i + 1
    best = None
    for i, (name, *_rest) in enumerate(RANKS):          # e.g. 'ProEggSnatcber': most shared words
        words = name.lower().split()
        hits = sum(w in (title or '').lower() for w in words)
        if hits >= 2 and (best is None or hits > best[0]): best = (hits, i + 1)
    return best[1] if best else None

def standing(title, points=None, stars=None):
    """The standing to record: {'tier', 'subtier', 'name', 'points', 'stars'}; sub-tier from points when known."""
    tier = tier_of(title)
    if tier is None: return None
    if tier == TOP_TIER:
        return {'tier': tier, 'subtier': 1, 'name': RANKS[-1][0], 'points': points, 'stars': stars}
    rows = [r for r in LADDER if r[0] == tier]
    row = rows[0]
    if points is not None:
        inside = [r for r in rows if r[3] <= points]
        row = inside[-1] if inside else rows[0]
    return {'tier': tier, 'subtier': row[1], 'name': row[2] if points is not None else RANKS[tier - 1][0],
            'points': points, 'stars': None}

def parse_points(text):
    """'Rank Points 1277/2370' -> (1277, 2370)."""
    m = re.search(r'(\d[\d,]*)\s*/\s*(\d[\d,]*)', (text or '').replace('O', '0').replace('o', '0'))
    return (int(m.group(1).replace(',', '')), int(m.group(2).replace(',', ''))) if m else (None, None)

# Season start dates (MM/DD/YYYY as shown under the season title) -> the game's season badge
KNOWN_SEASONS = {'2026-09-15': 'S1'}

def current_season(today=None):
    """The season running today, from the known start dates (for screens that don't name it)."""
    import datetime
    today = (today or datetime.date.today()).isoformat()
    started = [(start, name) for start, name in KNOWN_SEASONS.items() if start <= today]
    return max(started)[1] if started else None

def parse_gain(text):
    """'+18' (the Rank Points a match added, on the post-match rank screen) -> 18."""
    m = re.search(r'\+\s*(\d{1,4})', (text or '').replace('O', '0'))
    return int(m.group(1)) if m else None

def parse_season(badge, header=''):
    """'S1' from the season badge; the stylised badge rarely OCRs, so the date range under the season title is the
    usual source: a known start date gives its badge name, an unknown one 'season-YYYY-MM-DD'."""
    m = re.search(r'\b[S5$]\s*(\d{1,2})\b', badge or '')
    if m: return f'S{int(m.group(1))}'
    m = re.search(r'(\d{2})/(\d{2})/(\d{4})\s*-', (header or '').replace('O', '0'))
    if m:
        start = f'{m.group(3)}-{m.group(1)}-{m.group(2)}'
        return KNOWN_SEASONS.get(start, f'season-{start}')
    return None
