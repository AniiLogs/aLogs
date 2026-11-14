"""core.LogWatcher.feed (one compiled search over each new chunk, 14 Nov) gives the same events, in the same order, with
the same UID, current space and delays, as the per-line tests it replaced (copied below as the reference).

    python tests/log_watcher_same.py

The log is made up here from the patterns the watcher looks for (no player's log). It is fed whole, in small pieces
that cut lines (and the words) in two, and line by line. Exit code 1 on any difference.
"""
import collections, os, random, sys, time
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__))); sys.path.insert(0, HERE)
import core
from core import LogWatcher, uid_in

class Events(list):
    def put(self, e): self.append(e)

def old_feed(w, state, chunk):
    """The loop body of LogWatcher.run before 14 Nov, as it was."""
    lines = (state['buf'] + chunk).split(b'\n'); state['buf'] = lines.pop()
    for raw in lines:
        if b'callerId=' in raw or b'/prod/' in raw:             # service calls and login downloads
            uid = uid_in(raw)
            if uid and uid != w.uid:
                w.uid = uid; w.events.put(('uid', uid.decode(), None))
            if b'createVoxelScene' not in raw: continue
        if (b'createVoxelScene' not in raw and b'clearSpace' not in raw and b'bossrush' not in raw
                and b'fb_roguelike' not in raw):
            if b'][info]' in raw and len(w.delays) < 20: w._delay(raw.decode('utf-8', 'replace'))
            continue
        line = raw.decode('utf-8', 'replace'); d = w._delay(line)
        m = w.RX_ENTER.search(line)
        if m: w.current = m.group(1); w.events.put(('enter', m.group(1), d)); continue
        m = w.RX_LEAVE.search(line)
        if m: w.events.put(('leave', m.group(1), d)); continue
        if 'fb_bossrush' in line: w.events.put(('bossrush', w.current, d))
        elif 'fb_roguelike' in line: w.events.put(('holosim', w.current, d))

def made_up_log(n=3000, seed=7):
    rnd = random.Random(seed); t0 = 1_800_000_000; out = []
    for i in range(n):
        ts = time.strftime('%Y-%m-%d %H:%M:%S', time.localtime(t0 + i)) + f'.{rnd.randint(0, 999):03d}'
        stamp = rnd.choice([f'[{ts}]', f'[{ts}][info]', f'[{ts}][warn]', ''])
        uid = rnd.choice(['100000000001', '100000000002', '123456'])
        space = f'{rnd.randint(1, 9)}-{rnd.choice(["100000000001", "200000000002"])}-{rnd.choice([550, 1201, 3301])}'
        body = rnd.choice([
            f'net request callerId={uid} op=Get',
            f'download https://cdn.example/prod/ClientInfo/{uid}_client_info_bin ok',
            f'download https://cdn.example/prod/pet_jewelry/{uid} ok',
            f'download https://cdn.example/prod/other/file ok',
            f'createVoxelScene new>>>id:{space}',
            f'createVoxelScene new>>>id:{space} callerId={uid}',
            f'clearSpace is_destroy:true id:{space}',
            f'clearSpace is_destroy:false id:{space}',
            'load scene fb_bossrush_03',
            'load scene fb_roguelike_holo',
            'bossrush ui opened',
            'plain line with nothing in it',
            'info about ][info] in the middle',
            'unicode é中 text createVoxelScene new>>>id:',
            '',
        ])
        out.append(f'{stamp} {body}' if stamp else body)
    text = '\n'.join(out).encode('utf-8')
    return text + rnd.choice([b'', b'\n', b'\r\n', b'createVoxelScene new>>>id:unfinished'])

def run(feed, log_bytes, pieces, prefill=0):
    w = LogWatcher.__new__(LogWatcher)
    w.events = Events(); w.uid = None; w.current = None; w.delays = collections.deque(maxlen=20); w.buf = b''
    for k in range(prefill): w.delays.append(0.0)
    state = {'buf': b''}
    for p in pieces(log_bytes):
        feed(w, state, p)
    return list(w.events), w.uid, w.current, list(w.delays), (state['buf'] if feed is old_feed else w.buf)

def new_feed(w, state, chunk): w.feed(chunk)

def whole(b): return [b]
def lines(b): return [x + b'\n' for x in b.split(b'\n')[:-1]] + [b.split(b'\n')[-1]]
def small(seed):
    def cut(b):
        rnd = random.Random(seed); out = []; i = 0
        while i < len(b):
            n = rnd.randint(1, 120); out.append(b[i:i + n]); i += n
        return out
    return cut

fails = []
real_time = time.time
time.time = lambda: 1_800_000_000 + 1500.0           # _delay's "now", the same for both
try:
    for seed in range(6):
        log_bytes = made_up_log(seed=seed)
        for name, pieces in [('whole', whole), ('lines', lines), ('small', small(seed))]:
            for prefill in (0, 19, 20):
                a = run(old_feed, log_bytes, pieces, prefill); b = run(new_feed, log_bytes, pieces, prefill)
                if a != b:
                    fails.append(f'seed {seed}, {name}, delays prefilled {prefill}: '
                                 f'{len(a[0])} events vs {len(b[0])}, first difference at '
                                 f'{next((i for i, (x, y) in enumerate(zip(a[0], b[0])) if x != y), min(len(a[0]), len(b[0])))}')
                elif not a[0]: fails.append(f'seed {seed}, {name}: no events at all (the made-up log is wrong)')
finally:
    time.time = real_time
print(f'{6 * 3 * 3} feeds compared')
if fails:
    print('FAIL\n  ' + '\n  '.join(fails)); sys.exit(1)
print('PASS')
