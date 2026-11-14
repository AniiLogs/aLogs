"""reWASD (a controller remapper): the game reads reWASD's virtual controller, which HidHide can't hide, and a
controller the game already has open can't be hidden anyway. So while aLogs has the Holo-Battle Sim's end, the controller
is switched to a reWASD slot with nothing mapped, and back after, with reWASD's own command line (select_slot: the
virtual controller stays connected, so nothing reconnects; the owner, 14 Nov). Without reWASD, or not set up, nothing
here does anything and the meter uses its focus guard instead.

Set up in <data>/rewasd.json: {"id": "<device ID>", "slot": "slot1", "hold_slot": "slot4"}. The device ID: reWASD ›
right-click the controller › Copy device ID. hold_slot: a slot whose config maps nothing (every button unmapped).
"""
import json, logging, os, subprocess, threading

log = logging.getLogger('meter')
EXE = os.path.join(os.environ.get('ProgramFiles', r'C:\Program Files'), 'reWASD', 'reWASDCommandLine.exe')
SLOTS = ('slot1', 'slot2', 'slot3', 'slot4')
held = False


def config(data_dir):
    """The setup from <data>/rewasd.json, or None when it's missing, incomplete or reWASD isn't installed."""
    try:
        with open(os.path.join(data_dir, 'rewasd.json'), encoding='utf-8') as fh: c = json.load(fh)
    except (OSError, ValueError): return None
    if not (isinstance(c, dict) and c.get('id') and c.get('slot') in SLOTS and c.get('hold_slot') in SLOTS
            and c['slot'] != c['hold_slot'] and os.path.isfile(EXE)): return None
    return c


def _select(device, slot, why):
    try:
        r = subprocess.run([EXE, 'select_slot', '--id', str(device), '--slot', slot], capture_output=True, text=True,
                           timeout=5, creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
        out = (r.stdout or r.stderr or '').strip().replace('\n', ' ')[:160]
        log.info(f'reWASD: {slot} selected ({why}){"" if r.returncode == 0 else f" - exit {r.returncode}"}{f": {out}" if out else ""}')
    except (OSError, subprocess.SubprocessError) as e:
        log.info(f'reWASD: {slot} not selected ({why}): {e!r}')


def hold(c, on):
    """Switch the controller to the empty slot (on) or back to its own (off), without waiting for reWASD."""
    global held
    if not c or held == on: return False
    held = on
    threading.Thread(target=_select, args=(c['id'], c['hold_slot'] if on else c['slot'],
                                           'aLogs has control' if on else 'control given back'), daemon=True).start()
    return True
