"""The boss rush (Interlink) week schedule, read from the player's own game install.

The game ships its data tables in LuaScripts/Common/Data/pmdata.bin (a packed table format, decoded in
aniimo_data/decoder). Each game patch brings the coming week(s) in bossrush_cycle_data, with the three bosses as
species ids; bossrush_guanka_data maps those to the boss ids the meter and the site use. This reads only those two
tables, and only when pmdata.bin has changed (the result is cached in the data folder).
"""
import json, os, re, struct

DATA_REL = os.path.join('Aniimo_Data', 'cvs', 'res', 'lua', 'LuaScripts', 'Common', 'Data', 'pmdata.bin')
SIZE = {0: 8, 1: 1, 2: 1, 3: 2, 4: 2, 5: 4, 6: 4, 7: 4, 8: 4, 9: 1}
NUMKEY_SIZE = {0: 8, 5: 4, 3: 2, 1: 1}


class PmData:
    def __init__(self, path):
        d = self.d = open(path, 'rb').read()
        pos = 0; prev = b''; pool = []
        while True:                                   # the sorted string pool at the start of the file
            e = d.find(b'\0', pos); s = d[pos:e]
            if s < prev and len(pool) > 10: break
            try: s.decode('utf8')
            except UnicodeDecodeError: break
            pool.append(pos); prev = s; pos = e + 1
        self.end = pos; self.strings = set(pool) - {0}
        self.keysets = {}; self.classes = {}; self.consts = {}
        self._scan_keysets()

    def u32(self, q): return struct.unpack_from('<I', self.d, q)[0]
    def st(self, o): return self.d[o:self.d.find(b'\0', o)].decode('utf8', 'replace')
    def off_of(self, name):
        i = self.d.find(b'\0' + name.encode() + b'\0'); return i + 1 if i >= 0 else None

    def _scan_keysets(self):
        """Every key block: byte 1, u32 n keys, u32 array part, then n string offsets."""
        d, S, u32, q, L = self.d, self.strings, self.u32, self.end, len(self.d)
        find = d.find
        while True:
            q = find(b'\x01', q, L - 9)
            if q < 0: break
            n = u32(q + 1); a = u32(q + 5)
            if 0 < n < 400000 and a < 400000 and q + 9 + 4 * n <= L and u32(q + 9) in S:
                step = 1 if n < 5000 else max(1, n // 50)
                if all(u32(q + 9 + 4 * i) in S for i in range(0, n, step)):
                    self.keysets[q - 4] = (n, a, [u32(q + 9 + 4 * i) for i in range(n)])
                    q += 9 + 4 * n; continue
            q += 1

    def _numeric_keyset(self, K):
        d, u32 = self.d, self.u32
        if not (self.end < K < len(d) - 13) or d[K + 4] not in NUMKEY_SIZE: return None
        n = u32(K + 5)
        if not (0 < n < 400000 and u32(K + 9) == n): return None
        tag = d[K + 4]; w = NUMKEY_SIZE[tag]
        def num(q):
            if tag == 0: return struct.unpack_from('<d', d, q)[0]
            if tag == 5: return struct.unpack_from('<i', d, q)[0]
            if tag == 3: return struct.unpack_from('<H', d, q)[0]
            return d[q]
        return (n, 0, [('#', num(K + 13 + w * i)) for i in range(n)])

    def cls(self, t):
        if t in self.classes: return self.classes[t]
        d, u32 = self.d, self.u32
        result = None
        K, A, B = u32(t), u32(t + 4), u32(t + 8)
        if self.end < A < len(d) and self.end < B < len(d):
            if K not in self.keysets:
                numeric = self._numeric_keyset(K)
                if numeric: self.keysets[K] = numeric
            if K in self.keysets:
                n, a, keys = self.keysets[K]
                w = {1: 1, 3: 2, 7: 4}.get(d[B])
                types = list(d[A:A + n + a])
                if w and types and not any(9 < x < 0x64 or x > 0x6d for x in types):
                    offs = [int.from_bytes(d[B + 1 + w * i:B + 1 + w * (i + 1)], 'little') for i in range(n + a)]
                    cpos = {}; p = B - sum(SIZE[x - 0x64] for x in types if x >= 0x64)
                    for i, x in enumerate(types):
                        if x >= 0x64: cpos[i] = p; p += SIZE[x - 0x64]
                    self.consts[t] = cpos
                    result = (keys, a, types, offs)
        self.classes[t] = result
        return result

    def _val(self, at, ty):
        d = self.d
        if ty == 0: return struct.unpack_from('<d', d, at)[0]
        if ty == 1: return d[at]
        if ty == 2: return struct.unpack_from('<b', d, at)[0]
        if ty == 3: return struct.unpack_from('<H', d, at)[0]
        if ty == 4: return struct.unpack_from('<h', d, at)[0]
        if ty == 5: return struct.unpack_from('<i', d, at)[0]
        if ty == 6: return struct.unpack_from('<I', d, at)[0]
        if ty == 7: v = self.u32(at); return self.st(v) if v in self.strings or v == 0 else None
        if ty == 9: return bool(d[at])
        return None

    def tree(self, o, depth=0, maxdepth=6):
        d = self.d
        if not (self.end <= o < len(d) - 12) or not (self.end <= self.u32(o) < len(d) - 12): return None
        c = self.cls(self.u32(o))
        if c is None: return None
        keys, a, types, offs = c
        names = [k[1] if isinstance(k, tuple) else self.st(k) for k in keys] + [j + 1 for j in range(a)]
        out = {}
        for i, name in enumerate(names):
            ty = types[i]
            if ty >= 0x64: at = self.consts[self.u32(o)][i]; ty -= 0x64
            else: at = o + offs[i]
            out[name] = (self.tree(self.u32(at), depth + 1, maxdepth) if depth < maxdepth else None) if ty == 8 else self._val(at, ty)
        if a and not keys: return [out[j + 1] for j in range(a)]
        return out

    def table(self, name):
        """A top-level data table by name (the directory object is found by its keys, so patches can move it)."""
        k = self.off_of(name)
        for K, (n, a, keys) in self.keysets.items():
            if n < 1000 or k not in keys: continue
            i = keys.index(k)
            target = struct.pack('<I', K)
            p = self.d.find(target, self.end)
            while p != -1:                                   # classes using this key set, then an object of one
                c = self.cls(p)
                if c:
                    q = self.d.find(struct.pack('<I', p), self.end)
                    while q != -1:
                        if self.cls(self.u32(q)) is c:
                            return self.tree(self.u32(q + c[3][i]))
                        q = self.d.find(struct.pack('<I', p), q + 1)
                p = self.d.find(target, p + 1)
        return None


def find_game_data(player_log=None):
    """pmdata.bin in the game install (path from the game's own log, else the usual Steam folders)."""
    candidates = []
    if player_log and os.path.exists(player_log):
        try:
            with open(player_log, 'rb') as f: head = f.read(400_000).decode('utf-8', 'replace')
            candidates += [m.group(1) for m in re.finditer(r'([A-Za-z]:[/\\][^\r\n"]*?steamapps[/\\]common[/\\]Aniimo)', head)]
        except OSError: pass
    for drive in 'CDEFG':
        for base in (r'\Program Files (x86)\Steam', r'\Program Files\Steam', r'\SteamLibrary', r'\Steam'):
            candidates.append(drive + ':' + base + r'\steamapps\common\Aniimo')
    for c in candidates:
        p = os.path.join(c.replace('/', '\\'), DATA_REL)
        if os.path.exists(p): return p
    return None


def read_cycles(path):
    """[{'cycle', 'season', 'start', 'end', 'bossLv', 'bossIds': [left, middle, right]}] from the game data."""
    pm = PmData(path)
    cycles = pm.table('bossrush_cycle_data') or {}
    stages = pm.table('bossrush_guanka_data') or {}
    def boss_id(species):
        entry = stages.get(species) if isinstance(stages, dict) else None
        if entry is None and isinstance(stages, dict): entry = stages.get(str(species))
        return entry.get('bossId') if isinstance(entry, dict) else None
    out = []
    for key in sorted(cycles, key=lambda k: int(k)):
        c = cycles[key]
        if not isinstance(c, dict): continue
        beg, end = c.get('begDayTime') or {}, c.get('endDayTime') or {}
        ids = [boss_id(c.get(side)) for side in ('bossLeft', 'bossMid', 'bossRight')]
        if not all(ids): continue
        out.append({'cycle': str(int(key)), 'season': c.get('seasonId'), 'bossLv': c.get('bossLv'),
                    'start': beg.get(1, beg.get('1')), 'end': end.get(3, end.get('3')), 'bossIds': ids})
    return out


def cached_cycles(cache_path, player_log=None):
    """The schedule, decoded again only when the game's data file changed. Returns (cycles, changed) or (None, False)."""
    path = find_game_data(player_log)
    if not path: return None, False
    stamp = f'{os.path.getsize(path)}:{int(os.path.getmtime(path))}'
    try:
        with open(cache_path, encoding='utf-8') as f: cached = json.load(f)
        if cached.get('stamp') == stamp: return cached['cycles'], False
    except (OSError, ValueError, KeyError):
        pass
    cycles = read_cycles(path)
    if not cycles: return None, False
    with open(cache_path, 'w', encoding='utf-8') as f: json.dump({'stamp': stamp, 'cycles': cycles}, f)
    return cycles, True
