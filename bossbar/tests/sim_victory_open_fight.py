"""A won Holo-Battle Sim's last fight stays open while its bar is hidden (BAR_GONE_GRACE), and only the quick boxes are
read then. 3 Nov: the Victory screen was up for ~18 s inside that window, never read, and the win never sent. This reads
test_shots/sim_victory.webp the way the meter does with a fight open and its bar hidden (quick), in and out of a
Holo-Battle Sim, and checks the Victory line ('sv') is read in a Sim and still left out elsewhere.
Usage: python tests/sim_victory_open_fight.py"""
import os, sys
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__))); sys.path.insert(0, HERE)
from PIL import Image
import live as L
import reader as R

im = Image.open(os.path.join(HERE, 'test_shots', 'sim_victory.webp')).convert('RGB')
m = L.Meter.__new__(L.Meter)                    # only what screen_crops and read_screen's Victory branch use
failed = False
def check(name, ok):
    global failed
    print(('PASS ' if ok else 'FAIL ') + name); failed |= not ok

m.holosim_space = 'aupfFWp5cUmpz_8U'
crops = m.screen_crops(im, quick=True)
check("a Sim fight open, bar hidden: the Victory line is among the quick reads", 'sv' in crops)
ocr = R.Ocr()
try:
    text = ocr.read({'sv': crops['sv']}).get('sv', '') if 'sv' in crops else ''
finally:
    getattr(ocr, 'close', lambda: None)()
check(f"it reads as the Victory line ({text!r})", 'battle time' in text.lower())
m.holosim_space = None
check('outside a Sim the quick reads stay as they were (no Victory line)', 'sv' not in m.screen_crops(im, quick=True))
print('ALL PASS' if not failed else 'SOME FAILED'); sys.exit(1 if failed else 0)
