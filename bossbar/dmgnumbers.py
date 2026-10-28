"""Experimental: your own damage numbers (DPS Meter › Meter › Read my damage numbers).

Only the player's own damage numbers float over enemies (teammates' aren't shown), so reading them live gives solo
DPS during a fight; Battle Data stays the exact figure afterwards and is what the reader will be checked against.

Step 1 (this module): record the first seconds of fights, every frame, so the reader can be built and tested on
real sequences (numbers float up for about a second, overlap, and sit over effects). Only the middle of the screen
is kept, shrunk to about 1080p size by the graphics card, on a capture slot of its own so the meter's own reading
isn't slowed down.

Measured at 4K: converting and shrinking a frame ~16 ms, but saving a PNG ~90 ms (and it holds Python's lock, so
the recording stalled) and zlib ~65 ms (frees the lock, but ~4 cores at 60 a second). So during the burst each frame
is written as plain pixels (~3 MB at 4K, ~1.8 GB a burst), and afterwards one background thread compresses them.
Frames: logs/test_frames/<day>/numbers/<time>_<boss>/<frame>_<ms>_<w>x<h>.rgb.z (zlib'd RGB rows); load_frame()
reads one back.
"""
import glob, os, queue, re, threading, time, zlib
from PIL import Image
import wincapture as wc

AREA = (0.15, 0.08, 0.85, 0.80)   # where the numbers float: over the boss, the middle of the screen
BURST_S = 10                      # seconds recorded from the start of a fight
DAY_MAX = 3                       # fights recorded a day
SCALE_H = 1080                    # frames from bigger screens are shrunk by a whole factor towards this height
PACE = 1 / 60


def load_frame(path):
    """A recorded frame as a PIL image."""
    w, h = (int(v) for v in re.search(r'_(\d+)x(\d+)\.rgb', os.path.basename(path)).groups())
    with open(path, 'rb') as f: data = f.read()
    return Image.frombytes('RGB', (w, h), zlib.decompress(data) if path.endswith('.z') else data)


class Recorder:
    def __init__(self, here, log):
        self.here, self.log = here, log
        self.reader = wc.SharedCapture(tag=1)
        self.queue = queue.Queue(maxsize=120)        # ~2 s of frames waiting to be written at most
        self.thread = None; self.stop_at = 0.0
        threading.Thread(target=self._write, daemon=True, name='numbers-write').start()

    def fight_started(self, hwnd, boss):
        """A fight began: record its first BURST_S seconds, unless today's DAY_MAX are done or one is running."""
        day = os.path.join(self.here, 'logs', 'test_frames', time.strftime('%Y%m%d'), 'numbers')
        done = len(os.listdir(day)) if os.path.isdir(day) else 0
        if not hwnd or done >= DAY_MAX or (self.thread and self.thread.is_alive()): return
        folder = os.path.join(day, time.strftime('%H%M%S') + '_' + re.sub(r'[^A-Za-z0-9]+', '', boss or 'boss')[:30])
        os.makedirs(folder, exist_ok=True)
        self.stop_at = time.time() + BURST_S
        self.thread = threading.Thread(target=self._record, args=(hwnd, folder), daemon=True, name='numbers-record')
        self.thread.start()
        self.log.info(f'damage numbers: recording {BURST_S} s of {boss} ({done + 1} of {DAY_MAX} today)')

    def fight_ended(self):
        self.stop_at = 0.0

    def _record(self, hwnd, folder):
        last = None; n = dropped = 0; started = time.time(); factor = 0; self.shrink = 1
        while time.time() < self.stop_at:
            t = time.perf_counter()
            im = self.reader.read_area(hwnd, AREA, shrink=self.shrink)   # 2 at 4K: the card halves it
            when = getattr(self.reader, 'picture_time', None)
            if im is None or when == last:                       # nothing new yet: look again in 2 ms, not a frame
                time.sleep(0.002); continue
            if True:
                last = when
                if not factor:   # the first picture: how much to shrink (a whole factor, 2 at 4K), done by the card from now on
                    factor = max(1, int(im.height * self.shrink / (AREA[3] - AREA[1]) / SCALE_H + 0.25))
                    self.shrink = 4 if factor >= 4 else 2 if factor >= 2 else 1
                    if self.shrink > 1: continue
                if im.height / (AREA[3] - AREA[1]) > SCALE_H * 1.5: im = im.reduce(self.shrink)   # an app that can't
                try:
                    name = f'{n:04d}_{int(when * 1000)}_{im.width}x{im.height}.rgb'
                    self.queue.put_nowait((os.path.join(folder, name), im.tobytes())); n += 1
                except queue.Full:
                    dropped += 1
            time.sleep(max(0.0, PACE - (time.perf_counter() - t)))
        seconds = max(0.001, time.time() - started)
        self.log.info(f'damage numbers: recorded {n} frames in {seconds:.1f} s ({n / seconds:.0f}/s, {dropped} dropped)')
        self.queue.put((folder, None))                           # compress them once they're all written

    def _write(self):
        while True:
            path, data = self.queue.get()
            if data is None:
                threading.Thread(target=self._compress, args=(path,), daemon=True, name='numbers-compress').start()
                continue
            try:
                with open(path, 'wb') as f: f.write(data)
            except OSError:
                pass

    def _compress(self, folder):
        """After a burst, one file at a time (zlib frees Python's lock, so the meter isn't held up): ~3 MB -> ~2 MB."""
        for path in sorted(glob.glob(os.path.join(folder, '*.rgb'))):
            try:
                with open(path, 'rb') as f: data = f.read()
                with open(path + '.z', 'wb') as f: f.write(zlib.compress(data, 1))
                os.remove(path)
            except OSError:
                pass
