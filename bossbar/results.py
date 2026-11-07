"""Reading the game's end screens into runs (core, shared by the plug-ins): result screens, Battle Data, Holo-Battle
Sim's Victory, Egg Heist's end and rank screens, Challenge Stats; each run saved with its fight (logs/fights). The DPS
meter keeps them as your results; the leaderboards plug-in (leaderboards.py) decides what to send."""
from core import *


# Labels exactly as the game writes them (its English text table): a read is snapped to the closest one, so small
# text at 720p ("Tot al Da nn age") still records the game's wording.
RESULT_STATS = ('Total Damage Dealt', 'Total BREAK Damage', 'Targets Defeated', 'Most Healing', 'Most Damage Taken',
                'Most Hits Taken', 'Most Support Provided',
                'Best Strike Damage')     # the Interlink result screen's per-player highlight
# 'Best Strike Damage' (text id 1277896829) was left out: it stayed as read, so a slot with only that label read dropped
# out (321 slot reads of 31 Oct-1 Nov carry it, all with a value read so far); of the 2228
# logged label reads, only it and 2 misreads of it snap differently
EGG_MODES = ('Egg Heist Team Mode', 'Egg Heist')

def canon(text, choices, cutoff=0.6):
    """The game label `text` most likely is (letters only, case-insensitive), or `text` as read if none is close."""
    norm = lambda s: re.sub(r'[^a-z]', '', (s or '').lower())
    t = norm(text)
    if not t: return text
    best = max(choices, key=lambda c: difflib.SequenceMatcher(None, t, norm(c)).ratio())
    return best if difflib.SequenceMatcher(None, t, norm(best)).ratio() >= cutoff else text

def first_num(s):
    """The number in an OCR read of tiled short text ('60 60 60' -> 60): the one most tiles agree on, so one misread
    tile doesn't win by coming first ('6 36 36' was read as 6 stars)."""
    nums = re.findall(r'\d+', (s or '').replace('O', '0'))
    return int(max(nums, key=lambda n: (nums.count(n), -nums.index(n)))) if nums else None

# Holo-Battle Sim's stage line, as the game's rogue_difficulty_data names it (interfaceName). Season 1: "Training of
# Water - Hard" (Easy, "Common" for Normal, Hard) and "Trial of Fire - Challenge". Season 2 on: "Earth Training -
# Easy" ... "- Trial". Each season brings new elements (Grass/Water/Fire, then Earth/Lightning/Dark), so any word is
# taken as the element. Between the stage and its difficulty: " - ", which OCR sometimes reads with a stray letter.
SIM_DIFF = r'(Easy|Common|Normal|Hard|Challenge|Trial)(?![A-Za-z]{2})'   # ('Hardk': a stray letter after it)
SIM_STAGE_OF = re.compile(r'\b(Training|Trial|Crucible)\s*of\s*([A-Za-z]{3,12}).{0,6}?' + SIM_DIFF, re.I)
SIM_STAGE = re.compile(r'\b([A-Za-z]{3,12})\s*(Training|Trial).{0,6}?' + SIM_DIFF, re.I)

def parse_sim(text):
    """Holo-Battle Sim's Victory line "Battle time 00:08:22 Earth Training - Hard" (or Season 1's "Training of Water -
    Hard") -> {'time_s': 502, 'trial': 'Earth Training', 'element': 'Earth', 'difficulty': 'Hard'}, or None unless the
    time and the stage both read. The stage name is kept as the game shows it."""
    t = re.search(r'(\d{1,2})\s*[:.]\s*(\d{2})\s*[:.]\s*(\d{2})', text or '')
    if not t or int(t.group(2)) > 59 or int(t.group(3)) > 59: return None
    rest = (text or '')[t.end():]
    s = SIM_STAGE_OF.search(rest)
    if s: kind, element, difficulty = (g.capitalize() for g in s.groups()); trial = f'{kind} of {element}'
    else:
        s = SIM_STAGE.search(rest)
        if not s or s.group(1).lower() in ('time', 'battle'): return None
        element, kind, difficulty = (g.capitalize() for g in s.groups()); trial = f'{element} {kind}'
    return {'time_s': int(t.group(1)) * 3600 + int(t.group(2)) * 60 + int(t.group(3)),
            'trial': trial, 'element': element, 'difficulty': difficulty}

# Holo-Battle Sim's Victory screen (UI_Pb_Tower_Settlement), as fractions of the 16:9 UI area: the team on the left
# (the MVP's big card, then four smaller ones, each with "Lv. 60" over the name), the MVP's damage / damage taken /
# healing shares beside its card, the coins, and the Holo-gizmo series row with a count badge under each icon.
SIM_CARDS = ((0.135, 0.385, 0.26, 0.45), (0.13, 0.555, 0.27, 0.61), (0.268, 0.575, 0.385, 0.63),
             (0.13, 0.745, 0.25, 0.80), (0.248, 0.745, 0.37, 0.80))
SIM_SHARES = (0.352, 0.30, 0.40, 0.40)
SIM_COINS = (0.795, 0.285, 0.84, 0.325)
SIM_GIZMO_X, SIM_GIZMO_PITCH, SIM_GIZMO_Y = 0.464, 0.0588, (0.5795, 0.5985)
SIM_GIZMOS = 7
# Without a "Rewards Earned" row (no first clear: 3 Nov, Lightning Training - Trial) the right side's contents sit this
# much lower (fractions of H): the coins, the Data Collection pictures, the gizmo row and its counts. The team and
# shares on the left don't move. Both heights are read; the one that reads is kept (parse_sim_team 'low').
SIM_LOW = 0.0417

def sim_badges(im, u, ux, H, dy=0.0):
    """The digits on the Holo-gizmo badges (dark on light grey, too small for Windows OCR on their own), pasted in
    one line after a drawn label: 'n: / 6 / 6 / 2 / 2 / 4 / 5'. Empty slots (an X) give nothing."""
    from PIL import ImageDraw, ImageFont
    try: font = ImageFont.truetype('arialbd.ttf', 60)
    except OSError: font = ImageFont.load_default()
    k = u / 2000
    out = Image.new('L', (140 + SIM_GIZMOS * 110, 120), 255); ImageDraw.Draw(out).text((10, 25), 'n:', fill=0, font=font)
    for i in range(SIM_GIZMOS):
        cx = SIM_GIZMO_X + SIM_GIZMO_PITCH * i
        c = im.crop((int(ux + u * (cx - 0.0075)), int(H * (SIM_GIZMO_Y[0] + dy)), int(ux + u * (cx + 0.0075)), int(H * (SIM_GIZMO_Y[1] + dy)))).convert('L')
        c = c.resize((max(1, int(c.width * 4 / k)), max(1, int(c.height * 4 / k))), Image.LANCZOS).point(lambda v: 0 if v < 120 else 255)
        c.thumbnail((90, 90)); out.paste(c, (120 + i * 110, 15))
    return out

def sim_team_crops(im, u, ux, H):
    """The Victory screen's team, shares, coins and gizmo counts, prepared for one OCR call."""
    k = u / 2000; b = lambda x0, y0, x1, y1: (ux + u * x0, H * y0, ux + u * x1, H * y1)
    crops = {}
    for i, box in enumerate(SIM_CARDS):
        crops[f'c{i}w'] = R.prep_white(im, b(*box), k); crops[f'c{i}b'] = R.prep_bright(im, b(*box), k)
    crops['share'] = R.prep_white(im, b(*SIM_SHARES), k)
    crops['coins'] = R.prep_bright(im, b(*SIM_COINS), k)
    crops['gizmos'] = sim_badges(im, u, ux, H)
    x0, y0, x1, y1 = SIM_COINS                                    # ...and lower, when there's no Rewards Earned row
    crops['coinsL'] = R.prep_bright(im, b(x0, y0 + SIM_LOW, x1, y1 + SIM_LOW), k)
    crops['gizmosL'] = sim_badges(im, u, ux, H, SIM_LOW)
    return crops

def parse_sim_team(t, names=()):
    """What sim_team_crops() read -> {'team': [{'name', 'level', 'mvp'}], 'mvpShares': {'damage', 'taken', 'healing'},
    'gizmoCounts': [6, 6, 2, ...], 'coins': 8550}. A card's name is matched to the game's Aniimo names when given
    ('Ir?sahs' -> 'Irisalis'); a card that doesn't read is left out, so the team may be short but never wrong."""
    team = []
    for i in range(len(SIM_CARDS)):
        reads = []
        for v in (t.get(f'c{i}w'), t.get(f'c{i}b')):
            m = re.search(r'L\W?v\W*(\d{1,3})\W+(\S.{1,24})', v or '')
            if m and 1 <= int(m.group(1)) <= 100: reads.append((int(m.group(1)), m.group(2).strip()))
        if not reads: continue
        # the game name the card's reads come closest to (one read can be 'Ir?sahs', the other 'Irisalis'); else,
        # as the names come from the head portraits and a few Aniimo have none (Leafy), a read that is a plain name
        norm = lambda x: re.sub(r'[^a-z]', '', x.lower())
        score, name = max(((difflib.SequenceMatcher(None, norm(r), norm(n)).ratio(), n) for _, r in reads for n in names),
                          default=(0, None))
        if score < 0.7:
            plain = [r for _, r in reads if re.fullmatch(r"[A-Z][a-z]+(?:[ '-][A-Z][a-z]+)?", r)]
            if not plain: continue
            name = plain[0]
        team.append({'name': name, 'level': reads[0][0], 'mvp': i == 0})
    shares = []
    for part in (t.get('share') or '').split(' / '):      # OCR's line breaks (a '%' itself can read as '0/0')
        part = re.sub(r'\s*(%|[0oO]\s*/\s*[0oO]|[0oO]/[0oO])\s*$', '', part.strip())   # '%' reads as '0/0'
        n = re.findall(r'\d{1,3}', part)
        if n and int(n[-1]) <= 100: shares.append(int(n[-1]))
    def right(sfx):                                       # the coins and gizmo counts at one height
        coins = re.sub(r'[^\d]', '', t.get('coins' + sfx) or '')
        return ([int(n) for n in re.findall(r'\d{1,2}', (t.get('gizmos' + sfx) or '').split(':', 1)[-1])][:SIM_GIZMOS],
                int(coins) if coins and len(coins) <= 7 else None)
    (counts, coins), (countsL, coinsL) = right(''), right('L')
    low = (len(countsL), coinsL is not None) > (len(counts), coins is not None)
    if low: counts, coins = countsL, coinsL
    return {'team': team,
            'mvpShares': dict(zip(('damage', 'taken', 'healing'), shares)) if len(shares) == 3 else None,
            'gizmoCounts': counts, 'coins': coins, 'low': low}

def bd_complete(cards, party):
    """Battle Data read whole on its first look: a card for every player the Party screen had (when it was read), and
    each card's average DPS and BREAK. Damage taken and healing can be blank on the game's own card (none taken)."""
    return bool(cards) and len(cards) >= len(party or []) and all(
        c.get('avg_dps') is not None and c.get('break') is not None for c in cards)

def parse_battle_data(text):
    """Battle Data cards: OCR gives each card's 4 labels, then its 4 values in the same order
    (Average DPS, BREAK Value, Damage Taken, Heal Amount). A lone 'o'/'O' is a 0. Each card is read on its own,
    from one 'Average DPS' label to the next: a value the OCR missed (a blank Heal) used to shift every later card
    by one, so the next player's DPS became this one's heal. A card short of a value keeps the ones it has, in
    order, and the missing ones are None (never another player's): a card is kept even with one value, so the cards
    after it stay with their players. A downed player's card has no HP bar, and OCR reads its values on the label's
    line ('BREAK Value 2058717', '0 Damage Taken 21398': the 0 is the label's icon): a number ending a label is that
    label's value (1 Nov: the card was dropped and the next player's numbers moved onto this one). That card's lower
    lines can come after the next card's numbers, under their labels again ('O Damage Taken / O Heal Amount / 28605 /
    7737'): a label a card already had goes back to the nearest earlier card missing it, with the numbers after it."""
    keys = ('avg_dps', 'break', 'taken', 'heal'); cards = []
    def label(p):
        return 'break' if 'BREAK' in p else 'taken' if 'Damage' in p else 'heal' if 'Heal' in p else None
    for seg in re.split(r'[^/]*Average\s*D', text or '')[1:]:          # one segment per card, labels then values
        vals, seen, spill = [], set(), None                            # spill: [[key, value or None], ...]
        for part in (p.strip() for p in seg.split('/')):
            p = part.replace(',', '')
            if re.fullmatch(r'[oO]', p): p = '0'
            if re.fullmatch(r'\d+', p):
                if spill is not None:
                    gap = next((s for s in spill if s[1] is None), None)
                    if gap: gap[1] = int(p)
                else: vals.append(int(p))
            elif re.search(r'[A-Za-z]', p):
                key = label(p); m = re.search(r'\s(\d+)$', p)
                if key and key in seen and spill is None: spill = []
                if spill is not None:
                    if key: spill.append([key, int(m.group(1)) if m else None])
                    continue
                if key: seen.add(key)
                if m: vals.append(int(m.group(1)))
        card = dict(zip(keys, vals[:4] + [None] * (4 - len(vals[:4]))))
        cards.append(card)
        had = [sum(c[k] is not None for k in keys) for c in cards]       # counted before any spilled value lands
        for key, value in spill or []:      # to the shortest card missing it (the downed one), the nearest of those
            short = [i for i in range(len(cards) - 2, -1, -1) if cards[i][key] is None]
            owner = cards[min(short, key=lambda i: had[i])] if short else None
            if owner is not None and value is not None: owner[key] = value
    # a card with no number at all is a misread (the screen still opening, or half covered): no cards from this read,
    # rather than empty ones that would count as players
    if any(all(c[k] is None for k in keys) for c in cards): return []
    for card in cards:
        # a card one value short is missing its Heal: the lone '0' the OCR skips (every short card on 30 Oct, checked
        # against the proof pictures, was a Heal of 0)
        if sum(card[k] is not None for k in keys) == 3 and card['heal'] is None: card['heal'] = 0
    return cards

def vote(votes, key, value):
    """One read's value for `key` into `votes`; returns the value most reads so far agree on (the first seen on a tie;
    a read that found nothing doesn't vote)."""
    if value not in (None, ''): votes.setdefault(key, collections.Counter())[value] += 1
    c = votes.get(key)
    return max(c, key=lambda v: (c[v], -list(c).index(v))) if c else value

def sure_read(ocr, img):
    """Is this name box's read in the name cache for good? A read of one character or none only is when the Korean
    model vouched for it (Ocr.read_native: KoreanNames.lone, font-checked, or a Hangul name the font settled)."""
    hit = getattr(ocr, 'name_cache', {}).get(R.name_signature(img))
    return bool(hit) and hit[1] is None

def party_slots(players, party, sure=(), you=None):
    """Each result-screen (or Battle Data) column takes the Party screen's player of the same slot: both list players
    by badge, 1P to 4P, left to right (a run as 2P with three real players lined up on all three screens, 30 Oct). The
    name comes from the Party screen, read clearly there, with its AI flag and you; the column's own read only checks it.
    A misread like 'Dyn*ht' no longer has to be guessed at. A column that clearly reads as another member's name (the
    order wasn't what was expected) takes that member instead, and it's logged. No party: names stay as read.
    Each member goes to one column at most: first to a column that clearly reads their name, then to their slot's
    column; a column whose slot's member went to another keeps its own read, with no AI flag or you from the party.
    Given to both (by name to one, by slot to the other), one player was listed twice in 7 sent runs of 1 Nov (their
    Battle Data cards too), 2 of them with two rows marked as you. Names are compared folded (TEAM.fold: 0 as O, 1 as
    l): in 5 saved runs a column read a member's name that only folding matches, and stayed unmatched.
    Two columns keep their own read: one that reads as `you` (your character name, when the game menu showed it) is
    you, and no other column takes the Party screen's you; one of one Latin letter or digit the Korean model vouched
    for (`sure`: those slots, sure_read) is that player. The Party screen's read of either slot can only be worse: its
    filter never keeps a one-character name, and on its other layout (pills ~0.018 H higher, before everyone is ready)
    its boxes read the title above or half a pill. 6 of 59 sent runs had no row marked as you (the result screen read
    your exact name in all 6), and 4 of 10 saved runs of 1 Nov sent a one-character teammate's misread."""
    if not party:
        for p in players:
            if p.get('name') and not p.get('self'): p['name'] = TEAM.steady(p['name'])
        return
    by_slot = {m['slot']: m for m in party}
    sim = lambda a, b: difflib.SequenceMatcher(None, TEAM.fold(a), TEAM.fold(b)).ratio()
    taken, rest = set(), []                      # members (by slot) given to a column; the columns left to settle
    # the Party screen's you: its mark, or your name. Its vote keeps a slot's commonest name but the last read's mark,
    # so a shuffled read can leave your name unmarked in one slot and the mark on another (1 saved run)
    mine = {x['slot'] for x in party if x.get('self') or (you and same_name(x['name'], you))}
    def give(p, m):                              # a member taken as you takes every Party you: never two rows
        p.update(name=m['name'], ai=bool(m.get('ai')), self=bool(m.get('self')))
        taken.update(mine if m['slot'] in mine else {m['slot']})
    me = max((p for p in players if you and same_name(p.get('name'), you)), key=lambda p: sim(p['name'], you), default=None)
    for p in players:
        own, m = p.get('name') or '', by_slot.get(p['slot'])
        if p is me:
            if m and m['name'] != you: log.info(f"result: column {p['slot'] + 1} reads as you, not the Party screen's {m['name']!r}")
            p.update(name=you, ai=False, self=True); taken.update(mine)
        elif p['slot'] in sure and len(own) == 1 and own.isascii() and own.isalnum():
            if m and m['name'] != own: log.info(f"result: column {p['slot'] + 1} reads {own!r} (sure), not the Party screen's {m['name']!r}")
            p.setdefault('ai', False)
        else: rest.append(p)
    def named(p):                                # the free member this column clearly reads, when not its slot's
        m = by_slot.get(p['slot'])
        other = max((x for x in party if x is not m and x['slot'] not in taken), key=lambda x: sim(x['name'], p['name']), default=None)
        s = sim(other['name'], p['name']) if other else 0
        return (s, other) if s >= 0.8 and (m is None or s >= sim(m['name'], p['name']) + 0.3) else None
    while True:                                  # the clearest read first, when two columns read one member
        hits = [(h, p) for p in rest for h in [named(p)] if h]
        if not hits: break
        (s, other), p = max(hits, key=lambda h: h[0][0])
        m = by_slot.get(p['slot'])
        log.info(f"result: column {p['slot'] + 1} reads {p['name']!r}, the Party screen's {other['name']!r} (slot {other['slot'] + 1}), not slot {p['slot'] + 1}'s {m and m['name']!r}")
        give(p, other); rest.remove(p)
    for p in rest:
        m = by_slot.get(p['slot'])
        if m is None: continue
        if m['slot'] in taken:
            log.info(f"result: column {p['slot'] + 1} keeps its read {p['name']!r}: slot {p['slot'] + 1}'s {m['name']!r} is another column's")
            if p['name']: p['name'] = TEAM.steady(p['name'])
            continue
        if p['name'] and sim(m['name'], p['name']) < 0.3: log.info(f"result: column {p['slot'] + 1} reads {p['name']!r}, taken as slot {p['slot'] + 1}'s {m['name']!r}")
        give(p, m)

def mark_you(players, party, known):
    """After party_slots, on the result screen: your row is the one it marked, else the first that reads as your known
    name or as the Party screen's you; never a second one. Each row's AI flag, when party_slots gave none, is the Party's
    for its name."""
    you = next((m['name'] for m in party or [] if m.get('self')), None)
    mine = any(p.get('self') for p in players)
    for p in players:
        if not mine and ((known and same_name(p['name'], known)) or (you and p['name'] == you)): p['self'] = mine = True
        p['self'] = bool(p.get('self'))
        p.setdefault('ai', any(m['ai'] and m['name'] == p['name'] for m in party or []))

def own_name(known, names, party):
    """Which name on the result screen is the player running the meter. You aren't always the 1P slot (in someone
    else's party you're 2P-4P), so only a sure answer counts: your saved name is among the slots, or the prep screen
    showed every other member as AI, which leaves you as the one real player. None when it can't be told."""
    names = [n for n in names if n and 2 <= len(n) <= 40]
    if known and any(same_name(known, n) for n in names): return known
    humans = [m['name'] for m in party or [] if not m['ai']]
    if party and len(humans) == 1:
        return next((n for n in names if same_name(humans[0], n)), humans[0] if 2 <= len(humans[0]) <= 40 else None)
    return None

def remember_character(uid, name, verified=False):
    """Save the name for this UID. A name read from the game menu (verified) is never replaced by a guess from a
    result screen; a newer menu reading replaces it (renamed character)."""
    if not uid or not name: return
    known = _characters_file()
    old = known.get(uid, {})
    if old.get('verified') and not verified: return
    if old.get('name') == name and old.get('verified') == verified: return
    known[uid] = {'name': name, 'verified': verified}
    try:
        S.write_json(CHARACTERS, known, ensure_ascii=False)
    except OSError: pass
BATTLE_DATA_WAIT = 45               # seconds a best run waits for Battle Data before it goes with the result screen alone
BATTLE_DATA_SETTLE = 6              # ...then a run is decided this long after Battle Data is in, once those reads voted
SCREEN_READS = 10                   # reads of each screen per appearance; the value most reads agree on wins

SETTLE_S = 0.25         # s between looks while waiting for a proof screen to stop moving


class ResultsMixin:
    """Reading end screens into runs: the Meter's (live.py) methods for it, sharing its state."""

    @staticmethod
    def bd_card_count(im, u, ux, H):
        """How many cards Battle Data shows, counted from pixels: the light runs along a line between the BREAK and
        Damage Taken rows (y 0.645), each about a card wide (0.165-0.167 u on 55 real 4K captures) and centred where
        that many cards side by side would be (0.1943 u apart, as their boxes are placed). 0 when they don't line up
        like that (the screen sliding in, something in front of it). OCR's count put every box half a card off when it
        missed one: its first read found 3 of 4 cards on 14 of those 55 screens and none on 5 more; this found 4 on all
        55, scaled down to 720p as well (under 1 ms)."""
        y = int(H * 0.645); N = 400
        row = im.crop((int(ux), y, int(ux + u), y + 1)).convert('RGB').resize((N, 1), Image.BOX).tobytes()
        runs, start = [], None
        for x in range(N + 1):
            px = row[3 * x:3 * x + 3] if x < N else b'\0\0\0'
            light = min(px) >= 190 and max(px) - min(px) <= 40           # the card's pale grey, not the coloured scene
            if light and start is None: start = x
            elif not light and start is not None: runs.append((start / N, x / N)); start = None
        cards = [(a, b) for a, b in runs if b - a >= 0.12]
        n = len(cards)
        if not 1 <= n <= 4: return 0
        if any(b - a > 0.19 or abs((a + b) / 2 - (0.5 + (i - (n - 1) / 2) * 0.1943)) > 0.02 for i, (a, b) in enumerate(cards)):
            return 0
        return n

    def battle_data_font(self, im, u, ux, H, read):
        """Each card's values read digit by digit against the game's own digits, over OCR's in `read` (in place).
        OCR skipped 7-digit BREAK values, which moved the card's later numbers up a row (BREAK got Damage Taken's,
        and so on: 23 cards on 1 Nov). A card where a sure read disagrees with OCR keeps only the sure reads; its other
        values are left unread rather than sent from the wrong row."""
        reader = self.name_reader()
        if not reader: return
        n = len(read)
        for i, card in enumerate(read):
            c = ux + u * (0.5 + (i - (n - 1) / 2) * 0.1943)
            sure = {key: reader.number(im, (c + u * 0.015, H * (y - 0.016), c + u * 0.081, H * (y + 0.016)), dark=True, right=True)
                    for key, y in self.BD_ROWS}
            sure = {k: v for k, v in sure.items() if v is not None}
            slid = any(card.get(k) != v for k, v in sure.items())
            if slid: log.info(f'Battle Data card {i + 1}: OCR {card} disagrees with the game font {sure}: the font kept')
            for key, _ in self.BD_ROWS:
                card[key] = sure[key] if key in sure else (None if slid else card.get(key))

    def check_battle_data(self, im, now):
        """The game's screens with text: Battle Data, result screens, Catch Countdown, prep, Egg Heist, profile cards.
        Live, the text is read on its own thread (screen_bg) so a frame never waits for it: a new read starts as
        0.1 s after the last one (~40 ms) when the screen changed, about every 1.5 s when it didn't, and back to back -
        only the few boxes that matter then - while a fight's bar is hidden (the Catch Countdown dates the kill, see
        end_fight) or the automatic Battle Data click waits for its screen. What it read is handled here with the
        frame and time it was read from."""
        quick = self.auto_active() or (self.fight is not None and now - self.bar_seen > 0.5)
        bg = getattr(self, 'screen_bg', None)
        if bg is None:                                                  # replays and tests: read in place
            if now - self.last_results < (0.5 if quick else 1.5): return
            self.last_results = now
            crops = self.screen_crops(im, False)
            return self.read_screen(im, now, self.ocr.read(crops) if crops else {})
        if self.names_waiting(): return             # a screen waits for its players' names: it's handled first
        done = bg.take()
        if done: self.read_screen(done[1][0], done[1][1], done[0])
        if bg.busy or self.names_waiting() or not getattr(self, 'frame_full', True) or not self.screen_read_due(now, quick): return
        self.last_results = now; self.screen_read_sig = getattr(self, "top_sig", None)
        t0 = time.perf_counter()
        crops = self.screen_crops(im, quick)
        self.peak('prep', (time.perf_counter() - t0) * 1000)
        if crops:
            bg.submit(crops, (im, now))
            if getattr(self, 'load_counts', None): self.load_counts['reads'] += 1
        else: self.read_screen(im, now, {})                            # nothing on screen to read

    @staticmethod
    def bd_names(im, n, u, ux, H):
        """Battle Data's name tabs of n cards: prep_white's for Windows OCR and the Korean model, grey_crop's for the
        Latin model ({card: box} each)."""
        boxes = {i: (ux + u * (0.5 + (i - (n - 1) / 2) * 0.1943 - 0.045), H * 0.512,
                     ux + u * (0.5 + (i - (n - 1) / 2) * 0.1943 + 0.045), H * 0.547) for i in range(n)}
        return {i: R.prep_white(im, b, u / 2000) for i, b in boxes.items()}, {i: R.grey_crop(im, b) for i, b in boxes.items()}

    def battle_data(self, im, now, u, ux, H, head, names=None):
        """Battle Data on screen: each card's four values and its player, voted over the reads of one appearance, saved
        with the fight. `names`: bd_names' boxes, already read (names_first)."""
        if self.fight: self.end_fight('Battle Data screen')           # can show up while the bar dropout is still bridged
        # one appearance is one Battle Data (left open, or reopened from a menu, it isn't a new one for 30 s), read again
        # while it's up (SCREEN_READS in all, 0.5 s apart): each card value is the one most reads agree on
        shown_already = now - getattr(self, 'bd_seen', 0) < 6; self.bd_seen = now
        # fewer cards than the Party screen's players: read on (up to twice as often) rather than settle on them
        party = self.party_for_result(now) or []
        budget = SCREEN_READS * (2 if len(party) > getattr(self, 'bd_cards', 0) else 1)
        again = (shown_already and now - self.battle_saved < 30 and getattr(self, 'bd_reads', 0) < budget
                 and now - getattr(self, 'bd_last', 0) >= 0.5)
        if (shown_already or now - self.battle_saved < 30) and not again: return
        self.bd_last = now
        if not again:
            self.battle_saved = now; self.bd_reads = 0; self.bd_votes = {}; self.bd_shot = None; self.bd_cards = 0
            self.bd_prev_sig = None
            log.info(f'Battle Data screen detected (header OCR {head!r})')
        self.bd_reads += 1
        # the four stat rows of all cards (dark text on light cards)
        rows = im.crop((int(ux + u * 0.125), int(H * 0.564), int(ux + u * 0.875), int(H * 0.73))).convert('L')
        # smooth (LANCZOS) enlarging keeps the small 720p digits apart, so no card goes missing
        k = max(1.0, 2 * 2000 / u); rows = rows.resize((int(rows.width * k), int(rows.height * k)), Image.LANCZOS)
        text = self.ocr.read({'d': ImageOps.expand(rows, border=20, fill=255)}).get('d', '')
        read = parse_battle_data(text)
        log.info(f'Battle Data read {self.bd_reads}: OCR {text!r} -> {read}')   # whole: a run can be re-read from the log
        # the cards are counted on screen (bd_card_count), by OCR when they can't be or it found more. Either way the
        # count is the most any read found: a read never finds a card that isn't there (OCR needs each card's 'Average
        # DPS'), and settling on the commonest let one copy of a 4-player run keep 3 cards (31 Oct). A count on screen
        # under OCR's is cards it didn't see (the middle two of four sit where two cards would), not fewer cards
        before = getattr(self, 'bd_cards', 0); seen = self.bd_card_count(im, u, ux, H)
        n = self.bd_cards = min(4, max(before, seen, len(read)))
        read = [dict(read[i]) if i < len(read) else {} for i in range(n)]
        self.battle_data_font(im, u, ux, H, read)
        # the count and this read's numbers at it. A count off the party's is only logged: the game's panel
        # (Guis/Panels/BossRushBattleDetailResult) lists the run's player records, AI ones too, but where the cards sit
        # is in its prefab, and no 2- or 3-card screen has been seen yet
        if n != before:
            said = ('counted on screen' if seen >= len(read) else f"OCR's count (only {seen} counted on screen)") if seen \
                else "OCR's count (not counted on screen)"
            if party and len(party) != n: said += f', but the Party screen had {len(party)} players'
            log.info(f'Battle Data cards: {said} -> {n} cards {[{k: c.get(k) for k, _ in self.BD_ROWS} for c in read]}')
        cards = [{k: vote(self.bd_votes, (i, k), read[i].get(k)) for k in ('avg_dps', 'break', 'taken', 'heal')}
                 for i in range(n)]
        # Two reads in a row the same, number for number: the screen has settled and those are its numbers. A read
        # while Battle Data still fades in can miss a card's top row and slide the rest up (7 Nov: BREAK 511,876 went
        # out as average DPS); sent on one read, that went to the boards. The agreed read's numbers are the ones kept
        # (a 1-1 vote keeps the first read's, which was the wrong one).
        sig = tuple(tuple(c.get(k) for k in ('avg_dps', 'break', 'taken', 'heal')) for c in read)
        agreed = bool(read) and sig == getattr(self, "bd_prev_sig", None)
        self.bd_prev_sig = sig
        if agreed:
            for i, c in enumerate(cards): c.update({k: read[i].get(k) for k in ('avg_dps', 'break', 'taken', 'heal')})
        # each card's player: the name tab under the portrait (cards sit side by side, centred, in slot order 1P-4P),
        # then the Party screen's player of that slot (party_slots), so a card is tied to its player, not its position.
        # The Latin model reads the tab as it is on screen (grey_crop), Windows OCR and the Korean model prep_white's
        crops, grey = names if names and len(names[0]) == n else self.bd_names(im, n, u, ux, H)
        names = self.ocr.read(crops, names={i: 0 for i in crops}, raws=grey) if n else {}
        for i, c in enumerate(cards):
            c.update(slot=i, name=vote(self.bd_votes, (i, 'name'), (names.get(i) or '').strip(' /|') or None) or '')
        party_slots(cards, party, sure={i for i in crops if sure_read(self.ocr, crops[i])},
                    you=self.character_name if character_verified(self.uid) else None)
        vals = [c['avg_dps'] for c in cards if c['avg_dps'] is not None]
        lf = self.last_fight if self.last_fight and now - self.last_fight[1] < 180 else None
        if again:                                                  # a later read: only the agreed numbers change
            if lf: self.fight_summary(lf[0], battle_data={'players': cards, 'team_avg_dps': sum(vals) if vals else None,
                                                          'screenshot': self.bd_shot, 'reads': self.bd_reads})
            if agreed and bd_complete(cards, party):               # read twice the same: send, stop (below)
                if self.pending_run: self.pending_run = (self.pending_run[0], now)
                if getattr(self, 'auto_until', 0): self.end_auto('Battle Data read')
            return
        stamp = time.strftime('%Y%m%d_%H%M%S'); out_dir = os.path.join(HERE, 'logs', 'battle_data'); os.makedirs(out_dir, exist_ok=True)
        shot = self.picture(im, 'battle_data', stamp, proof=bool(lf and self.run_to_send(lf[0]))); self.bd_shot = os.path.relpath(shot, HERE) if shot else None
        msg = f'Battle Data screen saved: {os.path.relpath(shot, HERE) if shot else "numbers only (no picture kept)"}'
        if vals: msg += f' - game team DPS {sum(vals):,} (players {", ".join(f"{v:,}" for v in vals)})'
        f = self.awaiting[0] if self.awaiting and now <= self.awaiting[1] else None
        if f and vals:
            game_dps = sum(vals); T = f.elapsed; loss = f.damage / f.max_hp if f.max_hp else 0
            run = self.run_of(f)                                       # the run's damage: a two-boss fight's both bosses
            msg += f' vs meter {run["damage"] / T:,.0f} ({run["title"]} Lv {f.level})' if T > 1 else ''
            self.time_check(f, game_dps, run)
            calibrate = f.bossrush and f.started_at_top and loss > 0.02 and T > 60
            if calibrate and run['bosses'] is not None:
                # Never from a two-boss middle fight, whichever boss it ended on: Battle Data has one team DPS for the
                # run (BossRushBattleDetailResult; whether it covers both bosses isn't known until a live run shows it),
                # and no share of it is one boss's. Taken as the boss on the bar's, it put the second boss's max HP
                # ~1.2x too high, inside the guard below, and kept that for the boss from then on (calibration.json)
                calibrate = False
                log.info(f"Battle Data: no max HP measured from a two-boss fight ({', '.join(b['name'] for b in run['bosses'])}): "
                         "its team DPS is the run's, not one boss's")
            if calibrate:
                measured = game_dps * T / loss
                predicted = R.boss_max_hp(f.row, f.level, use_calibration=False)
            if calibrate and predicted and 0.5 <= measured / predicted <= 2:
                # far off the prediction means the wrong boss was matched, not a better HP figure: never save that
                kept = R.save_calibration(f.row['puppetId'], f.level, measured, predicted,
                                          f'{time.strftime("%Y-%m-%d %H:%M")} game {game_dps} T {T:.0f}')
                msg += f' | max HP measured {measured:,.0f} (predicted {predicted:,}); using {kept:,} from now on'
            self.awaiting = None
        self.out('status', msg)
        self.end_sig = screen_sig(im)
        if lf:
            self.fight_summary(lf[0], battle_data={'players': cards, 'team_avg_dps': sum(vals) if vals else None,
                                                   'screenshot': os.path.relpath(shot, HERE) if shot else None})
        # Both pictures are in (the result screen, then this): once two reads agree (above), the run goes now and the
        # automatic session stops looking (the inputs are let go). A card missing a number keeps reading and voting
        # as before (BATTLE_DATA_SETTLE). The owner, 7 Nov: result picture, Battle Data picture, send, stop.
        if agreed and bd_complete(cards, party):
            if self.pending_run: self.pending_run = (self.pending_run[0], now)
            if getattr(self, 'auto_until', 0): self.end_auto('Battle Data read')
        with open(os.path.join(out_dir, stamp + '.txt'), 'w', encoding='utf-8') as fh:
            fh.write(msg + '\n\n')
            for i, c in enumerate(cards, 1):
                fh.write(f"player {i}: average DPS {c['avg_dps']}, BREAK {c['break']}, damage taken {c['taken']}, heal {c['heal']}\n")
            fh.write('\nraw OCR of the stat rows:\n' + text.replace(' / ', '\n') + '\n')
            if f: fh.write(f'\nmeter fight log: {os.path.basename(f.log.name)}\n')

    def sanctum_in_run(self, now):
        """Sanctum Helper's live status says an Egg Heist run is on (it watches the run's map). Read about once a
        second; remembers when a run was last seen, since the run ends right as the screen fades."""
        if now - getattr(self, 'sh_checked', 0) >= 1:
            self.sh_checked = now
            path = os.path.join(os.environ.get('LOCALAPPDATA', ''), 'SanctumHelper', 'hosted', 'status.json')
            try:
                with open(path, encoding='utf-8') as fh: st = json.load(fh)
                if st.get('inRun') and time.time() * 1000 - float(st.get('time') or 0) < 5000: self.sh_run_seen = now
            except (OSError, ValueError, TypeError): pass
        return now - getattr(self, 'sh_run_seen', 0) < 30

    def sim_result(self, im, now, u, ux, H):
        """Holo-Battle Sim's end screen: "Victory" with "Battle time 00:05:25 Earth Training - Hard" under it (Season
        1: "Training of Water - Hard"). A win goes to the leaderboards with the screen as proof, and with the setup:
        the team (names and levels, the MVP's shares), the Holo-gizmo counts and coins from this screen, plus the
        Training Style and skills noted before the run (sim_prep_for)."""
        if now - getattr(self, 'sim_seen', 0) < 30: return                 # once per appearance
        box = (ux + u * 0.36, H * 0.198, ux + u * 0.65, H * 0.233)
        t = self.ocr.read({'w': R.prep_white(im, box, u / 2000), 'b': R.prep_bright(im, box, u / 2000),
                           'title': R.prep_bright(im, (ux + u * 0.43, H * 0.13, ux + u * 0.57, H * 0.20), u / 2000),
                           # (the gold title over the faint "SUCCESS" behind it: only its brightest part reads)
                           'title2': R.prep_text(im, (ux + u * 0.43, H * 0.13, ux + u * 0.57, H * 0.20), u / 2000, thr=200),
                           **sim_team_crops(im, u, ux, H)})
        sim = parse_sim(t.get('w')) or parse_sim(t.get('b'))
        # the pale gold title over a light background often doesn't read ('Victor', 'Vi<ory', nothing): a win is a
        # title that looks like Victory, or no 'Defeat' read and the team's cards reading as they do on a Victory
        title = f"{t.get('title') or ''} {t.get('title2') or ''}".lower()
        names = sorted({n for n, _ in self.icons.heads.values()}) if self.icons else ()
        team = parse_sim_team(t, names)
        won = bool(re.search(r'victo|v[il1]\W?[c<(]?t?ory', title)) or ('defe' not in title and len(team['team']) >= 3)
        if not sim:
            log.info(f"Holo-Battle Sim end screen not read: {t!r}"); return
        # The screen slides in: title and team first, the shares, coins and gizmo row a moment later (1 Nov: read
        # 0.3 s in, those were blank and the Gizmo screen never opened). Give them up to 3 s.
        if won and not (t.get('share') or t.get('coins') or t.get('coinsL')):
            first = getattr(self, 'sim_first', None)
            if first is None or now - first > 30: self.sim_first = first = now
            if now - first < 3: return
        self.sim_seen = now
        log.info(f"Holo-Battle Sim: {'victory' if won else 'not a victory'} ({t.get('title')!r}), {sim['trial']} - "
                 f"{sim['difficulty']}, battle time {sim['time_s']} s")
        if not won: self.sim_release('not a victory'); return
        # The same win still up (the game left on the Victory screen, tabbed out): it was sent once already. The
        # 30 s above only spaces the reads; this run went to the leaderboards four times before (1 Nov).
        key = (self.space, sim['trial'], sim['difficulty'], sim['time_s'])
        if key == getattr(self, 'sim_last', None): return
        self.sim_last = key
        low = team.pop('low', False)
        pictures = self.sim_pictures(im, u, ux, H, SIM_LOW if low else 0.0)
        if 'gizmos' not in pictures:
            other = self.sim_pictures(im, u, ux, H, 0.0 if low else SIM_LOW)
            if 'gizmos' in other: pictures, low = other, not low
        self.sim_low = low                                        # the tour clicks the gizmo row at that height
        sim['setup'] = {**team, **pictures, **self.sim_prep_for(sim)}
        counts = sim['setup'].pop('gizmoCounts')
        if sim['setup'].get('gizmos') and len(sim['setup']['gizmos']) == len(counts):
            sim['setup']['gizmos'] = [{**g, 'count': n} for g, n in zip(sim['setup']['gizmos'], counts)]
        elif sim['setup'].get('gizmos'):
            # the series known from their icons, a count under one not read (1 Nov: 6 icons, 5 counts): the series are
            # kept, so the Gizmo screen is still read (it lists each one), with the counts unknown rather than shifted
            log.info(f"Holo-Battle Sim: {len(sim['setup']['gizmos'])} gizmo series but {len(counts)} counts read {counts}: counts left out")
            sim['setup']['gizmos'] = [{**g, 'count': None} for g in sim['setup']['gizmos']]
        else: sim['setup']['gizmos'] = [{'count': n} for n in counts] or None
        log.info(f"Holo-Battle Sim setup: {sim['setup']}")
        shot = self.picture(im, 'screens', 'sim_' + time.strftime('%Y%m%d_%H%M%S'), proof=True)
        self.sim_pending = (sim, shot)
        if not self.sim_tour_start(now): self.sim_send()           # (gives control back too, if the gate took it)

    def sim_pictures(self, im, u, ux, H, dy=0.0):
        """The Victory screen's pictures, matched to the game's own (icons.py): the Training Style, from the Core
        gizmo the run started with (Valor's Fervor Mask); the Alpha Transformation (Scorchhowl); and which series
        each icon of the Holo-gizmo row is, left to right (the game lists them in series order, skipping empty ones).
        Only clear matches are kept; {} without the game's icons."""
        ic = self.icons
        if not ic or not ic.sim_art:
            log.info(f"Holo-Battle Sim pictures: not read (the game's icons {self.icons_state or 'not loaded'})"); return {}
        b = lambda cx, cy, r: (ux + u * (cx - r), H * (cy + dy) - u * r, ux + u * (cx + r), H * (cy + dy) + u * r)
        out = {}
        core = ic.sim_picture(im, b(0.571, 0.41, 0.04), 'core')
        if core and core[1] >= 0.3 and core[2] >= 0.1:
            out['style'] = ic.sim[core[0]].get('name'); out['core'] = ic.sim[core[0]].get('core')
        tf = ic.sim_picture(im, b(0.472, 0.41, 0.032), 'transform')
        if tf and tf[1] >= 0.35 and tf[2] >= 0.1: out['transform'] = ic.sim[tf[0]].get('transform')
        row = []
        for i in range(SIM_GIZMOS):
            cx = SIM_GIZMO_X + SIM_GIZMO_PITCH * i
            g = ic.sim_glyph(im, (ux + u * (cx - 0.02), H * (0.512 + dy), ux + u * (cx + 0.02), H * (0.578 + dy)))
            if g is None: break
            if g[1] < 0.45 or g[2] < 0.05 or (row and int(g[0]) <= int(row[-1])): row = None; break   # unsure / out of order
            row.append(g[0])
        if row: out['gizmos'] = [{'series': ic.sim[s].get('name'), 'sid': s} for s in row]
        log.info(f'Holo-Battle Sim pictures{" (lower)" if dy else ""}: core {core}, transform {tf}, series row {row}')
        return out

    def sim_prep_for(self, sim):
        """Each Aniimo's skills noted before this run (the Party screen), if seen in the last 2 hours. {} when not.
        (The Training Style comes from the Victory screen's Core gizmo.)"""
        prep = getattr(self, 'sim_prep', None)
        if not prep or time.time() - prep.get('at', 0) > 7200: return {}
        return {k: v for k, v in prep.items() if k == 'skills'}

    def egg_result(self, im, now, u, ux, H, title):
        """Egg Heist end screen ("Adventure Successful", or the failed one): difficulty, mode, time, objective, Loot
        This Match, Total Defeats and today's chests. One record per match in logs/egg_matches.csv, with the
        screen kept as proof."""
        if now - getattr(self, 'egg_seen', 0) < 3: return
        self.egg_seen = now
        k = u / 2000
        box = lambda a, b, c, d: (ux + u * a, H * b, ux + u * c, H * d)
        t = self.ocr.read({
            'difficulty': R.prep_bright(im, box(0.035, 0.2, 0.088, 0.232), k),
            'mode': R.prep_text(im, box(0.09, 0.2, 0.235, 0.232), k),
            'time': R.prep_short(im, box(0.262, 0.2, 0.31, 0.235), k),
            'goal': R.prep_text(im, box(0.05, 0.258, 0.26, 0.292), k),
            'chests': R.prep_bright(im, box(0.18, 0.81, 0.305, 0.84), k)})
        # Loot This Match and Total Defeats sit 4% higher when there's no objective line above them (Team Mode has
        # none): read them where this screen's layout puts them, and the other way if that gives no number.
        def numbers(dy):
            return self.ocr.read({
                'loot': R.prep_text(im, box(0.03, 0.345 + dy, 0.15, 0.42 + dy), k),
                'defeats': R.prep_labelled(im, box(0.03, 0.465 + dy, 0.075, 0.535 + dy), k, 'Defeats:'),
                'defeats2': R.prep_labelled(im, box(0.03, 0.465 + dy, 0.075, 0.535 + dy), k, 'Defeats:', size=3),
                'defeats3': R.prep_labelled(im, box(0.03, 0.465 + dy, 0.075, 0.535 + dy), k, 'Defeats:', size=5)})   # 720p
        shifts = (0.0, -0.04) if (t.get('goal') or '').strip() else (-0.04, 0.0)
        t.update(numbers(shifts[0]))
        if first_num((t.get('loot') or '').replace(',', '').replace(' ', '')) is None: t.update(numbers(shifts[1]))
        low = title.lower()
        outcome = 'success' if 'success' in low else 'failed' if ('fail' in low or 'unsuccess' in low) else title.strip()
        m = re.search(r'(\d{1,2})\s*:\s*(\d{2})', t.get('time', ''))
        seconds = int(m.group(1)) * 60 + int(m.group(2)) if m else None
        loot = first_num((t.get('loot') or '').replace(',', '').replace(' ', ''))
        # the number after the drawn label; a lone 1 often reads as I, l or |, a 0 as O
        def digits(text):
            raw = (text or '').split(':', 1)[-1].strip().translate(str.maketrans('Il|!Oo', '111100')).replace(' ', '')
            return int(raw) if re.fullmatch(r'\d+', raw) else None
        defeats = digits(t.get('defeats'))
        if defeats is None: defeats = digits(t.get('defeats2'))
        if defeats is None: defeats = digits(t.get('defeats3'))
        cm = re.search(r'(\d+)\s*/\s*(\d+)', t.get('chests', ''))
        difficulty = next((d for d in ('Chaos', 'Nightmare', 'Expert', 'Hard', 'Normal', 'Easy') if d.lower() in t.get('difficulty', '').lower()),
                          (t.get('difficulty') or '').strip() or None)
        mode = (t.get('mode') or '').split(':', 1)[-1].strip()
        match = {'outcome': outcome, 'difficulty': difficulty, 'mode': canon(mode, EGG_MODES) if mode else None,
                 'time_s': seconds, 'objective': (t.get('goal') or '').strip() or None, 'loot': loot, 'defeats': defeats,
                 'chests_today': f'{cm.group(1)}/{cm.group(2)}' if cm else None}
        log.info(f'Egg Heist result: OCR {t!r} -> {match}')
        if loot is None and seconds is None: return                  # still sliding in: the next frame reads it
        self.egg_hold_end('end screen read')
        key = (outcome, loot, seconds, defeats)
        if self.last_scores.get('egg_result') == key: return
        self.last_scores['egg_result'] = key
        shot = self.picture(im, 'screens', 'egg_result_' + time.strftime('%Y%m%d_%H%M%S'), proof=True)
        path = os.path.join(HERE, 'logs', 'egg_matches.csv'); new = not os.path.exists(path)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, 'a', newline='', encoding='utf-8') as f:
            w = csv.writer(f)
            if new: w.writerow(['time', 'uid', 'outcome', 'difficulty', 'mode', 'time_s', 'objective', 'loot', 'defeats', 'chests_today', 'screenshot'])
            w.writerow([time.strftime('%Y-%m-%d %H:%M:%S'), self.uid, outcome, difficulty, match['mode'], seconds, match['objective'],
                        loot, defeats, match['chests_today'], os.path.relpath(shot, HERE) if shot else ''])
        # to the leaderboards (Egg Heist > Matches), with the end screen as proof
        if self.submitter and self.uid:
            self.submitter.enqueue(S.egg_match_payload(match, getattr(self, 'egg_season', None), self.uid),
                                   os.path.relpath(shot, HERE) if shot else None)
        self.screen_settled('egg_result', time.time())
        self.out('status', f"Egg Heist match recorded: {'escaped' if outcome == 'success' else outcome}"
                 + (f', {loot:,} loot' if loot is not None else '') + (f' ({difficulty})' if difficulty else ''))

    def egg_rank_info(self, im, now, u, ux, H):
        """Egg Heist "Rank Info": rank title, "Rank Points X/Y" (below Egg Emperor) or the star count (at Egg
        Emperor), and the season badge. Recorded as seen; the sub-tier (IV, III, ...) comes from the points."""
        if now - self.stats_seen < 5: return
        self.stats_seen = now
        k = u / 2000
        t = self.ocr.read({
            'title': R.prep_text(im, (ux + u * 0.10, H * 0.195, ux + u * 0.365, H * 0.275), k),
            'points': R.prep_text(im, (ux + u * 0.105, H * 0.80, ux + u * 0.36, H * 0.86), k),
            # the star count (Egg Emperor) under the badge: Windows OCR is picky about the margins around it, so three
            # boxes are read and the first one that gives a number counts (checked at 1080p and 4K)
            'stars': R.prep_short(im, (ux + u * 0.215, H * 0.715, ux + u * 0.29, H * 0.785), k),
            'stars2': R.prep_short(im, (ux + u * 0.215, H * 0.715, ux + u * 0.30, H * 0.785), k),
            'stars3': R.prep_short(im, (ux + u * 0.225, H * 0.72, ux + u * 0.28, H * 0.78), k),
            'badge': R.prep_short(im, (ux + u * 0.918, H * 0.06, ux + u * 0.962, H * 0.125), k),
            'season': R.prep_text(im, (ux + u * 0.67, H * 0.05, ux + u * 0.91, H * 0.13), k)})
        points, next_start = E.parse_points(t.get('points'))
        stars = next((n for n in (first_num(t.get(key)) for key in ('stars', 'stars2', 'stars3')) if n is not None), None)             if points is None else None
        st = E.standing(t.get('title'), points, stars)
        # the season title can miss a frame (it fades in): then today's season by date, never an entry without one
        season = E.parse_season(t.get('badge'), t.get('season')) or getattr(self, 'egg_season', None) or E.current_season()
        if season: self.egg_season = season
        log.info(f'Egg Heist Rank Info: OCR {t!r} -> {st}, season {season}, next big rank at {next_start}')
        if not st or (st['points'] is None and st['stars'] is None): return
        key = (season, st['name'], st['points'], st['stars'])
        if self.last_scores.get('egg_rank') == key: return
        self.last_scores['egg_rank'] = key
        d = os.path.join(HERE, 'logs', 'screens'); os.makedirs(d, exist_ok=True)
        shot = self.picture(im, 'screens', 'egg_rank_' + time.strftime('%Y%m%d_%H%M%S'), proof=True)
        path = os.path.join(HERE, 'logs', 'scores.csv'); new = not os.path.exists(path)
        with open(path, 'a', newline='', encoding='utf-8') as f:
            w = csv.writer(f)
            if new: w.writerow(['time', 'uid', 'cycle', 'screen', 'boss', 'score', 'stars', 'screenshot'])
            w.writerow([time.strftime('%Y-%m-%d %H:%M:%S'), self.uid, season, 'egg_rank', st['name'],
                        st['points'], st['stars'], os.path.relpath(shot, HERE) if shot else ''])
        self.screen_settled('egg_rank', time.time())
        self.out('status', f"Egg Heist rank recorded: {st['name']}"
                 + (f" - {st['points']:,} rank points" if st['points'] is not None else '')
                 + (f" - {st['stars']:,} stars" if st['stars'] is not None else ''))
        if self.submitter:
            self.submitter.enqueue(S.eggheist_payload(st, season, self.uid, self.egg_profile(season, nextRankAt=next_start)),
                                   os.path.relpath(shot, HERE) if shot else None)

    def egg_profile(self, season, **values):
        """The season's numbers for the player's profile, kept in logs/egg_profile.json so the rank screens add to them
        (Rank Info's threshold, the post-match screen's gain and bar) instead of each sending only its own."""
        path = os.path.join(HERE, 'logs', 'egg_profile.json')
        try:
            with open(path, encoding='utf-8') as f: kept = json.load(f)
        except (OSError, ValueError): kept = {}
        mine = kept.setdefault(f'{self.uid}|{season}', {})
        mine.update({k: v for k, v in values.items() if v is not None})
        try:
            S.write_json(path, kept)
        except OSError: pass
        return dict(mine)

    def egg_rank_gain(self, im, now, u, ux, H, points_text):
        """Egg Heist's rank screen after a match: the rank title over the badge, the Rank Points bar "1337/1350" (the
        total and where the next egg fills) and what the match added ("+18"). The total updates the season's ladder
        entry; the bar and the gain go to the player's profile. The match's grade (B) is left out on purpose."""
        if now - getattr(self, 'egg_gain_seen', 0) < 5: return
        self.egg_gain_seen = now
        k = u / 2000
        box = lambda a, b, c, d: (ux + u * a, H * b, ux + u * c, H * d)
        t = self.ocr.read({'title': R.prep_bright(im, box(0.38, 0.215, 0.62, 0.265), k),
                           'title2': R.prep_text(im, box(0.37, 0.20, 0.63, 0.275), k, thr=230),
                           'gain': R.prep_text(im, box(0.85, 0.555, 0.90, 0.605), k)})
        points, bar_end = E.parse_points(points_text)
        gain = E.parse_gain(t.get('gain'))
        title = t.get('title') if E.tier_of(t.get('title')) else t.get('title2')
        st = E.standing(title, points)
        season = getattr(self, 'egg_season', None) or E.current_season()
        log.info(f'Egg Heist rank after a match: OCR {t!r}, {points_text!r} -> {st}, +{gain}, bar to {bar_end}, season {season}')
        if not st or points is None or gain is None: return
        key = (season, points, gain)
        if self.last_scores.get('egg_gain') == key: return
        self.last_scores['egg_gain'] = key
        self.out('status', f"Egg Heist: +{gain} Rank Points ({st['name']}, {points:,})")
        if self.submitter:
            profile = self.egg_profile(season, lastGain=gain, bar=[points, bar_end], barAt=int(time.time()))
            self.submitter.enqueue(S.eggheist_payload(st, season, self.uid, profile), None)

    def egg_emperor_count(self, im, now, u, ux, H, count_text):
        """The same post-match screen at Egg Emperor, the top rank: no points bar, just the eggs collected ("x57"),
        which keep adding up as you play. They're the season's standing at that rank (the ladder sorts by rank, then
        eggs)."""
        if now - getattr(self, 'egg_gain_seen', 0) < 5: return
        self.egg_gain_seen = now
        k = u / 2000
        box = lambda a, b, c, d: (ux + u * a, H * b, ux + u * c, H * d)
        t = self.ocr.read({'title': R.prep_text(im, box(0.37, 0.20, 0.63, 0.275), k, thr=230),
                           'title2': R.prep_bright(im, box(0.38, 0.215, 0.62, 0.265), k)})
        title = t.get('title') if E.tier_of(t.get('title')) else t.get('title2')
        eggs = first_num(count_text)
        st = E.standing(title, None, eggs) if E.tier_of(title) == E.TOP_TIER else None
        season = getattr(self, 'egg_season', None) or E.current_season()
        log.info(f'Egg Heist rank after a match (Egg Emperor): OCR {t!r}, {count_text!r} -> {st}, season {season}')
        if not st or eggs is None: return
        key = (season, 'emperor', eggs)
        if self.last_scores.get('egg_gain') == key: return
        self.last_scores['egg_gain'] = key
        self.out('status', f"Egg Heist: Egg Emperor, {eggs:,} eggs")
        if self.submitter:
            # at the top rank there's no bar or next rank any more: the eggs (stars) are the whole standing
            profile = {k: v for k, v in self.egg_profile(season).items() if k not in ('bar', 'barAt', 'lastGain', 'nextRankAt')}
            self.submitter.enqueue(S.eggheist_payload(st, season, self.uid, profile), None)

    def challenge_stats(self, im, now, u, ux, H):
        """Esc menu in boss rush: 'Challenge Stats' with this week's Best Score (sum of the best score on each boss)
        and Current Best stars. Recorded to logs/scores.csv (one row per new value) with a screenshot."""
        if now - self.stats_seen < 5: return
        self.stats_seen = now
        k = u / 2000
        text = self.ocr.read({'score': R.prep_text(im, (ux + u * 0.31, H * 0.325, ux + u * 0.455, H * 0.415), k, size=1),
                              'stars': R.prep_short(im, (ux + u * 0.555, H * 0.33, ux + u * 0.62, H * 0.41), k)})
        num = lambda s: int(re.sub(r'\D', '', s.replace('O', '0').replace('o', '0'))) if re.search(r'\d', s or '') else None
        score, stars = num(text.get('score')), first_num(text.get('stars'))
        log.info(f'Challenge Stats screen: OCR {text!r} -> best score {score}, stars {stars}')
        if score is not None: self.record_score(im, 'challenge_stats', 'all (weekly best)', score, stars)

    def party_for_result(self, now):
        """The party read on the prep screen of this run (recent enough to be the same run), or None."""
        return self.party['members'] if self.party and now - self.party['at'] < 900 else None

    @staticmethod
    def result_names(im, u, ux, H):
        """The result screen's four player-name boxes: prepared for Windows OCR and the Korean model, and grey_crop's
        for the Latin model ({'name0'..'name3': box} each)."""
        crops, grey = {}, {}
        for i in range(4):
            dx = i * 0.1765
            # a soft threshold keeps thin letters (no split or lost first letters): 130 reads every name at 720p,
            # 900p, 1080p, 1440p and 4K; 160 lost a 'T' at 720p and 1080p. As wide as the name pill can get (0.07 cut
            # a player's long name at both ends 25 times on 1 Nov), and below the stat number above it
            # The Latin model reads the box as it is on screen (grey_crop): 195 of 195 labelled names on real 4K result
            # screens, 180 from this preparation; scaled to 720p-1440p 194-195, where it was 158-188
            box = (ux + u * (0.13 + dx), H * 0.618, ux + u * (0.24 + dx), H * 0.655)
            crops[f'name{i}'] = R.prep_text(im, box, u / 2000, thr=130); grey[f'name{i}'] = R.grey_crop(im, box)
        return crops, grey

    def click_when_ready(self, im, u, ux, H):
        """The result screen is wholly read: the automatic Battle Data click is made with this frame, at once. Waiting
        for the next read needs a new picture, and on 6 Nov the app's capture stalled 18 and 28 s right after the first
        read (the screen was complete on it), so the click came 22 s late, then not at all."""
        auto = getattr(self, 'auto_active', None)
        if auto and auto() and getattr(self, 'clicks', 1) == 0 and self.args.replay is None:
            self.open_battle_data(im, u, ux, H)

    def result_screen(self, im, now, u, ux, H, banner, names=None):
        """Fight result right after a boss: Challenge Rating (stars), Challenge Score, Completion Time and one
        highlight stat per player. It doesn't name the boss, so it is tied to the fight the meter just tracked.
        Rows go to logs/results.csv; a 'New Best Record' also updates that boss in scores.csv. `names`: result_names'
        boxes, already read (names_first)."""
        # read again while it's up (SCREEN_READS in all, 0.4 s apart; the Battle Data click often ends it after one or
        # two): each value is the one most reads agree on, and later reads only update the run's numbers
        again = now - self.result_seen < 30
        if again and (getattr(self, 'result_reads', 0) >= SCREEN_READS or now - getattr(self, 'result_last', 0) < 0.45): return
        if not again: self.result_seen = now; self.result_reads = 0; self.result_votes = {}; self.result_shot = None
        self.result_last = now; self.result_reads += 1
        if self.fight: self.end_fight('result screen')
        k = u / 2000; crops = {
            'rating': R.prep_short(im, (ux + u * 0.20, H * 0.33, ux + u * 0.275, H * 0.41), k),     # short: tiled
            'score': R.prep_text(im, (ux + u * 0.43, H * 0.325, ux + u * 0.57, H * 0.415), k, size=1),
            'time': R.prep_short(im, (ux + u * 0.68, H * 0.33, ux + u * 0.79, H * 0.41), k)}
        named, grey = names or self.result_names(im, u, ux, H)
        for i in range(4):
            dx = i * 0.1765
            crops[f'name{i}'] = named[f'name{i}']
            crops[f'stat{i}'] = R.prep_text(im, (ux + u * (0.21 + dx), H * 0.575, ux + u * (0.33 + dx), H * 0.625), k, thr=160)
        t = self.ocr.read(crops, names={f'name{i}': 24 for i in range(4)}, raws=grey)
        num = lambda s: int(re.sub(r'\D', '', s)) if re.search(r'\d', s or '') else None
        V = lambda key, value: vote(self.result_votes, key, value)
        # the score digit by digit against the game's font when it reads clearly (OCR's 1 can come out as a 7)
        reader = self.name_reader()
        sure = reader.number(im, (ux + u * 0.43, H * 0.325, ux + u * 0.57, H * 0.415)) if reader else None
        # the stars too (OCR read 17 as 7 at 900p and 1080p); the gold star after them isn't white, so it's not read
        stars = reader.number(im, (ux + u * 0.20, H * 0.33, ux + u * 0.275, H * 0.41)) if reader else None
        # and the time, less the 's' the screen writes after the seconds (BossRushBattleResultCtrl.initUI): with the 's'
        # the font read none of 60 real result screens, without it all 60, the same at 720p-1440p
        sure_time = reader.number(im, (ux + u * 0.68, H * 0.33, ux + u * 0.79, H * 0.41), unit=True) if reader else None
        rating = V('rating', stars if stars is not None else first_num(t.get('rating')))
        score = V('score', sure if sure is not None else num(t.get('score')))
        secs = V('time', sure_time if sure_time is not None else first_num(t.get('time')))
        f = self.last_fight if self.last_fight and now - self.last_fight[1] < 180 else None
        # the result is the run's: a two-boss fight's under its stage's first boss, its damage both bosses' (run_of)
        run = self.run_of(f[0]) if f else None
        full = clear_stars(run['id'] if f else None)
        # the stars are a number only: the screen prints the run's grade (BossRushBattleResultCtrl.initUI), and no client
        # Lua shows bossrush_grade_data's grade names ("Eliminate"). A two-boss fight's score is left as read
        score = interlink_score(score, rating, full, two_boss=bool(run and run['bosses'] is not None))
        players = []
        for i in range(4):
            dx = i * 0.1765
            name = V(f'name{i}', (t.get(f'name{i}') or '').strip(' /|'))
            parts = [p.strip() for p in (t.get(f'stat{i}') or '').split(' / ') if p.strip()]
            # the value under the stat's label by the game's font first, else OCR (the player card writes the stat floored,
            # digits only: BossRushBattleResultCtrl, the stats from bossrush_fun_data). On 60 real result screens OCR
            # left 10 values unread (7 a lone 1, Targets Defeated; 23 in the first reads saved then) and cut 2 short; the
            # font read every one. 8 digits are more than number() takes, so those stay OCR's
            sure_value = reader.number(im, (ux + u * (0.21 + dx), H * 0.600, ux + u * (0.33 + dx), H * 0.624)) if reader else None
            value = V(f'value{i}', sure_value if sure_value is not None else
                      next((num(p) for p in reversed(parts) if re.fullmatch(r'[\d,.\s]+', p)), None))
            label = V(f'label{i}', canon(next((p for p in parts if not re.fullmatch(r'[\d,.\s]+', p)), ''), RESULT_STATS))
            # a slot is a player when its name, its value or its stat label was read: a slot with its label read but
            # neither of the others dropped out, and that player and their Battle Data card weren't sent (5 sent runs,
            # 31 Oct-1 Nov; 38 of 178 slot reads dropped in the logs had the label). party_slots gives it the Party
            # screen's player of that slot. The label never reads where no player is: 0 of 540 boxes off the cards
            # (beside the row and in the bands above and below it) on 60 saved result screens, 196 of 232 on the
            # cards of the 58 4K ones in a single read
            if name or value is not None or label in RESULT_STATS:
                players.append({'slot': i, 'name': name, 'stat': label, 'value': value})
        party_slots(players, self.party_for_result(now), sure={i for i in range(4) if sure_read(self.ocr, named[f'name{i}'])},
                    you=self.character_name if character_verified(self.uid) else None)
        mine = self.character_name if character_verified(self.uid) else \
            own_name(self.character_name, [p['name'] for p in players], self.party_for_result(now))
        if mine and mine != self.character_name:
            log.info(f'character for uid {self.uid}: {mine}')
            self.character_name = mine
            remember_character(self.uid, mine)
            if self.submitter: self.submitter.link_character(self.uid, mine)
        elif not mine:
            log.info('result screen: not sure which slot is you (a party with other players, and your name not known yet), '
                     'so the character name is left as it is')
        # A clear: finished before the clock ran out with the full rating (clear_stars: 20 left and right, 40 in the
        # center; it was 15 before the Whisperwake update), and the game's time is exact. That is every star under the
        # clock, not proof of a kill: a solo player or the leader can End a run at any stars
        # (QuitBtnUIComponent.onQuitBtnClick; the HUD's own end button shows at full stars), and a wipe ends it too
        # (BossRushConst.LevelEndType: LeaderExit, AllPlayerDie, BossDie, TimeOver). Under the clock with fewer stars
        # is no clear. A two-boss middle fight is one run with one rating, capped as the center (40), and one clock, the
        # first boss's (BossRushInfoUIComponent.setBossRushStar; level graph 78824533 nodes 5-11): its clear is the same
        # test. Whether killing both always shows 40 is for a live run to show; the screen's call stands either way.
        left = (f[0].max_hp - f[0].damage) / f[0].max_hp if f and f[0].max_hp else 0
        if run and run['bosses'] is not None:                          # of the boss on the bar, its own max HP
            left = f'{(f[0].max_hp - f[0].damage) / f[0].boss_hp if f[0].boss_hp else 0:.0%} of {f[0].title}, boss {f[0].order} of 2,'
        else: left = f'{left:.0%}'
        # The clock is the run's own (f.limit: Overtime makes it 4:00), else --duration (180,
        # sys_config_data.BossRushCombatDuration). The screen shows min(the clock, ceil(the run's time))
        # (BossRushBattleResultCtrl.initUI), so a time-up reads the clock exactly. Every star at the clock's end is no
        # kill: a team at full stars keeps hitting for score until time runs out.
        limit = ((f[0].limit or None) if f else None) or self.overtime_limit(now) or self.args.duration
        cleared = bool(f and secs is not None and 0 < secs < limit and f[0].bossrush and (rating or 0) >= full)
        if cleared: f[0].elapsed = secs; log.info(f'result screen: cleared in {secs}s')
        elif f and secs is not None and secs < limit:
            log.info(f'result screen: ended at {secs}s without a clear (rating {rating}, {left} HP left on the meter)')
        # the leader ended it ("The team leader has ended the challenge.", seen as the boss went): the site sorts out
        # what that makes the run (ended early with every star, or not counted)
        ended_by_leader = bool(f and getattr(self, 'leader_ended', None) is f[0])
        # the run's boss: a two-boss fight's first, the one its result screen names (BossRushBattleResultCtrl.initUI)
        boss = run['title'] if f else '(unknown - meter saw no fight)'
        log.info(f'result screen read {self.result_reads}: banner {banner!r}, OCR {t!r} -> {boss}: rating {rating}, score {score}, time {secs}, players {players}')
        # everything on it read (the proof was taken on the first read): the Battle Data click needn't wait any longer
        if rating is not None and score is not None and secs is not None and players and                 all(p.get('value') is not None for p in players) and not getattr(self, 'result_ready', 0) >= self.result_seen:
            self.result_ready = now
            self.click_when_ready(im, u, ux, H)
        if again:                                                  # a later read: only the agreed numbers change
            mark_you(players, self.party_for_result(now), self.character_name)
            if f: self.fight_summary(f[0], result={'new_best': 'best' in banner.lower(), 'rating_stars': rating, 'score': score,
                                                   'time_s': secs, 'cleared': cleared, 'star_cap': full, 'players': players, 'ended_by_leader': ended_by_leader,
                                                   'screenshot': self.result_shot, 'reads': self.result_reads})
            return
        d = os.path.join(HERE, 'logs', 'screens'); os.makedirs(d, exist_ok=True)
        shot = self.picture(im, 'screens', 'result_' + time.strftime('%Y%m%d_%H%M%S'), proof=True); self.result_shot = os.path.relpath(shot, HERE) if shot else None
        path = os.path.join(HERE, 'logs', 'results.csv'); new = not os.path.exists(path)
        with open(path, 'a', newline='', encoding='utf-8') as fh:
            w = csv.writer(fh)
            if new: w.writerow(['time', 'uid', 'cycle', 'boss', 'level', 'new_best', 'rating_stars', 'score', 'time_s',
                                'players', 'meter_damage', 'meter_seconds', 'meter_dps', 'meter_csv', 'screenshot'])
            fi = f[0] if f else None; dmg = run and run['damage']           # the run's damage, over the run's time
            w.writerow([time.strftime('%Y-%m-%d %H:%M:%S'), self.uid, R.active_cycle_id(), boss, fi and fi.level,
                        'best' in banner.lower(), rating, score, secs, json.dumps(players, ensure_ascii=False),
                        dmg, fi and round(fi.elapsed), fi and round(dmg / fi.elapsed) if fi and fi.elapsed > 1 else None,
                        fi and os.path.basename(fi.log.name), os.path.relpath(shot, HERE) if shot else ''])
        party = self.party_for_result(now)
        mark_you(players, party, self.character_name)
        party_shot = None
        if f and party and self.party.get('picture') is not None:   # the Party screen this run started from
            party_shot = self.picture(self.party['picture'], 'screens', 'party_' + time.strftime('%Y%m%d_%H%M%S'), proof=True, fresh=False)
        if f: self.fight_summary(f[0], result={'new_best': 'best' in banner.lower(), 'rating_stars': rating, 'score': score,
                                               'time_s': secs, 'cleared': cleared, 'star_cap': full, 'players': players, 'ended_by_leader': ended_by_leader, 'screenshot': os.path.relpath(shot, HERE) if shot else None},
                                 **({'party': party} if party else {}),
                                 **({'team': self.party.get('team'), 'teams': self.party.get('teams'), 'buffs': self.party.get('buffs')} if party else {}),
                                 **({'party_screenshot': os.path.relpath(party_shot, HERE)} if party_shot else {}))
        if 'best' in banner.lower() and score is not None and f: self.record_score(im, 'new_best', boss, score, rating)
        self.screen_settled('result', time.time())
        self.out('status', f'result recorded: {boss} - score {score}, {rating} stars, {secs}s')
        self.end_sig = screen_sig(im)                           # the note stays only while this screen does

    def run_of(self, f):
        """The run a Fight tracked, as the game keeps it: {'id', 'title' (its boss), 'damage', 'max_hp', 'bosses'}.
        A one-boss fight is its own boss, damage and max HP, with bosses None. A two-boss middle fight is one run under
        its stage's bossId, the first boss, whichever boss it ended on: the result screen names that boss alone and the
        run's best is kept under its id (BossRushBattleResultCtrl.initUI, line 60; BossRushMainCtrl.renderBossInfo,
        line 350). Its damage and max HP are both bosses' as far as the run got, and bosses lists each in order: its
        max HP, damage, whether it went down, and its time on the run's clock (from_s / to_s, seconds: from its bar's
        first sight to its last hit seen, as it died, or to the run's end for the last one; the gap between the bosses
        counts to neither) with its DPS over that time. A run first seen at its second boss (the meter started late)
        counts the first one down at its full max HP: the second spawns only on the first one's death (level graph
        78824533, nodes 16-19); its time is unknown (None)."""
        run = {'id': f.row['puppetId'], 'title': f.title, 'damage': f.damage, 'max_hp': f.max_hp, 'bosses': None}
        if not f.bossrush or getattr(f, 'order', None) is None: return run
        bosses = []
        for i, p in enumerate(f.phases):
            to = round(f.elapsed, 1) if i == len(f.phases) - 1 else p['end_s']     # the last one ends with the run
            secs = round(to - p['start_s'], 1) if to is not None and p['start_s'] is not None else None
            bosses.append({'id': p['row']['puppetId'], 'name': p['title'], 'max_hp': p['max_hp'], 'damage': p['damage'],
                           'defeated': p['defeated'], 'from_s': p['start_s'], 'to_s': to, 'seconds': secs,
                           'dps': round(p['damage'] / secs) if secs and secs > 1 else None, 'breaks': p['breaks']})
        first = f.phases[0]; run.update(id=first['row']['puppetId'], title=first['title'], bosses=bosses)
        _, order, other = R.stage_of(first['row']['puppetId'])
        row = next((r for r in self.catalog if r['table'] == 'puppet_data' and r['puppetId'] == other
                    and r['fixed_hp_rate']), None) if order == 2 else None
        if row:
            hp = R.boss_max_hp(row, f.level)
            bosses.insert(0, {'id': other, 'name': row['bossTitle_en'], 'max_hp': hp, 'damage': hp, 'defeated': True,
                              'from_s': None, 'to_s': None, 'seconds': None, 'dps': None, 'breaks': None, 'seen': False})
            run.update(id=other, title=row['bossTitle_en'] or first['title'], damage=f.damage + hp, max_hp=f.max_hp + hp)
        return run

    def fight_summary(self, f, **parts):
        """logs/fights/<fight>.json: the meter's numbers plus whatever game screens belong to this fight
        (result screen, Battle Data), merged as they appear. A two-boss middle fight's is the run's (run_of)."""
        d = os.path.join(HERE, 'logs', 'fights'); os.makedirs(d, exist_ok=True)
        path = os.path.join(d, os.path.splitext(os.path.basename(f.log.name))[0] + '.json')
        try:
            with open(path, encoding='utf-8') as fh: s = json.load(fh)
        except (OSError, ValueError):
            run = self.run_of(f)
            s = {'boss': run['title'], 'puppetId': run['id'], 'level': f.level, 'uid': self.uid, 'cycle': R.active_cycle_id(),
                 'started': time.strftime('%Y-%m-%d %H:%M:%S', time.localtime(f.t0)), 'bossrush': f.bossrush,
                 # the dungeon's own id from the game log: the same for everyone in that run, so two players' apps
                 # sending the run can be told apart from two unrelated runs with the same score
                 'instance': self.space if f.bossrush and self.space and scene_of(self.space) is None else None,
                 'interlink_scene': bool(getattr(f, 'scene_ok', False)),
                 'server': S.game_server(),
                 # the run's damage and DPS over the run's time: a two-boss fight's both bosses (run_of)
                 'meter': {'max_hp': run['max_hp'], 'damage': run['damage'], 'seconds': round(f.elapsed, 1),
                           'dps': round(run['damage'] / f.elapsed) if f.elapsed > 1 else None, 'segment_left': f.seg,
                           'tracked_from_start': f.started_at_top, 'csv': os.path.basename(f.log.name),
                           'time_limit_s': f.limit or self.overtime_limit(time.time()),
                           'breaks': sum(p['breaks'] for p in f.phases[:-1]) + f.brk.breaks,
                           'break_gauge_points': None if not f.brk.size else round(f.brk.size),
                           'break_rate_pct_s': None if f.brk.rate(time.time())[0] is None
                           else round(f.brk.rate(time.time())[0], 2),
                           # a two-boss stage's bosses in order, as far as the run got (run_of)
                           **({'bosses': run['bosses']} if run['bosses'] is not None else {})}}
        s.update(parts)
        S.write_json(path, s, indent=1, ensure_ascii=False)
        # leaderboard: a run goes, once, when it beats one of your bests on this boss this week (see beats_best),
        # with its result screen and Battle Data. Battle Data's own stats count too, so it decides once that's in.
        if self.submitter and s.get('result') and f.bossrush and 'submit' not in s and not s.get('interlink_scene'):
            log.info("leaderboard: the game log didn't confirm an Interlink scene for this fight - kept in your records, not sent")
            s['submit'] = False
            S.write_json(path, s, indent=1, ensure_ascii=False)
        if self.submitter and s.get('result') and f.bossrush and 'submit' not in s:
            self.schedule_run(path, bool(s.get('battle_data')), time.time())

    def schedule_run(self, path, battle_data, now):
        """When a finished run is decided and sent (check_pending_run): BATTLE_DATA_SETTLE after its Battle Data is in
        (more reads of both screens vote meanwhile), else BATTLE_DATA_WAIT after its result. Each read of either screen
        comes back here: the deadline only moves earlier (Battle Data arriving), never later - or the run waited until
        the player left the screens (20-30 s after the result, 5-6 Nov), and its "sent" notice with it."""
        due = now + (BATTLE_DATA_SETTLE if battle_data else BATTLE_DATA_WAIT)
        if self.pending_run and self.pending_run[0] == path: due = min(due, self.pending_run[1])
        self.pending_run = (path, due)

    def record_score(self, im, screen, boss, score, stars):
        """One row in logs/scores.csv per new value (plus a screenshot in logs/screens/), for the leaderboard."""
        if self.last_scores.get(boss) == (score, stars): return
        self.last_scores[boss] = (score, stars)
        d = os.path.join(HERE, 'logs', 'screens'); os.makedirs(d, exist_ok=True)
        shot = self.picture(im, 'screens', f'{screen}_' + time.strftime('%Y%m%d_%H%M%S'),
                            proof=screen in ('challenge_stats', 'battle_prep'))
        path = os.path.join(HERE, 'logs', 'scores.csv'); new = not os.path.exists(path)
        with open(path, 'a', newline='', encoding='utf-8') as f:
            w = csv.writer(f)
            if new: w.writerow(['time', 'uid', 'cycle', 'screen', 'boss', 'score', 'stars', 'screenshot'])
            w.writerow([time.strftime('%Y-%m-%d %H:%M:%S'), self.uid, R.active_cycle_id(), screen, boss,
                        score, stars, os.path.relpath(shot, HERE) if shot else ''])
        self.out('status', f'score recorded: {boss} - best score {score:,}' + (f', {stars} stars' if stars is not None else ''))
        if self.submitter and screen in ('challenge_stats', 'battle_prep'):
            row = None
            if screen == 'battle_prep':
                row = self.week_row(boss)
                # (it also needed the game log's Interlink scene, but since the Whisperwake update the prep screen
                # opens out in the world, before the run's scene: all 7 bests on 30 Oct were refused for that)
                if not row:
                    log.info(f"leaderboard: prep-screen best for {boss!r} not sent - not one of this week's bosses")
                    return
                boss = row['bossTitle_en']
            self.submitter.enqueue(S.score_payload(screen, boss, score, stars, self.uid, R.active_cycle_id(),
                                                   row['puppetId'] if row else None), os.path.relpath(shot, HERE) if shot else None)

    def week_row(self, name):
        """This week's Interlink boss with this name (from the boss list), or None."""
        active = R.active_cycle_bosses()
        return next((r for r in self.catalog if r['table'] == 'puppet_data' and r['bossTitle_en'] == name
                     and int(r['puppetId']) in active), None)

    def hide_end_note(self):
        if self.end_sig is not None: self.out('end_hide', None); self.end_sig = None

    def leader_ended_note(self, im, now):
        """Interlink: the game's line when the party leader ends a run early. It shows top center as the boss goes, so it
        is looked for while an Interlink fight's bar is hidden or just gone (about once a second): a quick check for a
        line of white text over the dark strip first, the OCR only then. The run then goes with extra.endedByLeader."""
        if now - getattr(self, 'leader_look', 0) < 1.0 or not getattr(self, 'frame_full', True): return
        f = self.fight or (self.last_fight[0] if self.last_fight else None)     # last_fight: (Fight, ended at)
        if not f or not f.bossrush or (not self.fight and now - getattr(self, 'fight_ended_at', 0) > 20): return
        self.leader_look = now
        W, H = im.size; u = min(W, H * 16 / 9); ux = (W - u) / 2; x0, y0, x1, y1 = self.LEADER_NOTE
        box = (ux + u * x0, H * y0, ux + u * x1, H * y1)
        c = im.crop(tuple(int(v) for v in box)).convert('L'); hist = c.histogram()
        white = sum(hist[230:]) / max(1, c.width * c.height)      # the line: ~4% white; open sky or sand: none or lots
        if not 0.01 <= white <= 0.15 or ImageStat.Stat(c).mean[0] > 140: return
        t = self.ocr.read({'n': R.prep_white(im, box, u / 2000)}).get('n') or ''
        if re.search(r'leader\W+has\W+ended|ended\W+the\W+challenge', t, re.I):
            if getattr(self, 'leader_ended', None) is not f: log.info(f'Interlink: the team leader ended the challenge ({t!r})')
            self.leader_ended = f

    def watch_end_note(self, im):
        """Every frame between fights: hide the end-screen note the moment the screen under it changes (a click,
        Battle Data opening, the screen closing) instead of waiting ~1 s for the next text read to say what's up."""
        if self.end_sig is not None and screen_changed(screen_sig(im), self.end_sig): self.hide_end_note()

    def picture(self, im, folder, name, proof=False, fresh=True):
        """A screen the meter just read. Its data goes into the records (CSV, JSON); the picture is only kept when
        Captures & logs > Keep screenshots says so, or - as leaderboard proof - until it has been sent (the submitter
        deletes it then). None when no picture is kept. A proof of the screen on show now (fresh) is a settled,
        full-size grab (settled_frame), not the frame it was spotted in, which may be mid-animation; a picture kept
        from earlier (the Party screen) is saved as it is."""
        keep = int(self.args.cfg['capture'].get('keep_hours', 0))
        sending = proof and self.submitter and self.submitter.config and self.submitter.config['screenshots']
        if keep == 0 and not sending: return None
        if proof and fresh: im = self.settled_frame(im)
        path = save_picture(im, folder, name, 3840 if sending else 1920)
        if proof: self.proof_with_overlay(im, name)
        return path

    def settled_frame(self, im):
        """The screen once it has stopped moving: a fresh whole frame every SETTLE_S until two in a row match (at most
        three looks, about 0.75 s), so a proof isn't caught sliding in or fading. The frame it was spotted in when a
        look fails, or in a replay."""
        if self.args.replay is not None: return im
        # proofs are the game alone: the overlay steps out of captures (poll_tick) before the first look, so nothing of
        # ours is in the picture and nothing is blacked out (Windows 10 shows it in captures the rest of the time)
        self.proof_until = time.time() + 3
        try: wc.shared.set_clean_until((time.time() + 3) * 1000)      # Sanctum Helper and the app's button too
        except Exception: pass
        try: return self._settled(im)
        finally:
            self.proof_until = time.time() + 0.1      # back in captures once the proof is taken
            try: wc.shared.set_clean_until((time.time() + 0.1) * 1000)
            except Exception: pass

    def _settled(self, im):
        last = im; sig = first = screen_sig(im) if im is not None and not im.info.get('parts') else None
        for _ in range(3):
            time.sleep(SETTLE_S)
            try: fresh = self.grab(False, False)
            except Exception: fresh = None
            if fresh is None: break
            fsig = screen_sig(fresh)
            # the screen that was read is gone (the game moved on): keep the last picture of it, never the next screen
            # (a Battle Data proof was a loading screen, 6 Nov: the session's time ran out while it settled)
            if first is not None and screen_changed(fsig, first): break
            last = fresh
            if sig is not None and not screen_changed(fsig, sig, limit=4): break      # two in a row match: settled
            sig = fsig
        return last

__all__ = ['BATTLE_DATA_SETTLE', 'bd_complete', 'BATTLE_DATA_WAIT', 'EGG_MODES', 'RESULT_STATS', 'SCREEN_READS', 'SETTLE_S', 'SIM_CARDS', 'SIM_COINS', 'SIM_DIFF', 'SIM_GIZMOS', 'SIM_GIZMO_PITCH', 'SIM_GIZMO_X', 'SIM_GIZMO_Y', 'SIM_LOW', 'SIM_SHARES', 'SIM_STAGE', 'SIM_STAGE_OF', 'canon', 'first_num', 'mark_you', 'own_name', 'parse_battle_data', 'parse_sim', 'parse_sim_team', 'party_slots', 'remember_character', 'sim_badges', 'sim_team_crops', 'sure_read', 'vote', 'ResultsMixin']
