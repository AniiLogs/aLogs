"""aLogs' plug-ins as the meter process sees them (plugins.json, the list the app reads too): whose switch is on, who
needs the game read, who owns which notice, and each plug-in's health - an error in one plug-in's code is caught and
logged under its name, and one that keeps failing is stopped (the others go on) until its switch is turned off and on
again or aLogs restarts."""
import collections, functools, json, logging, os, time
import settings as C

log = logging.getLogger('meter')


def _load():
    for folder in (C.RES, os.path.dirname(os.path.abspath(__file__))):
        try:
            with open(os.path.join(folder, 'plugins.json'), encoding='utf-8') as f: return json.load(f)
        except (OSError, ValueError): pass
    raise RuntimeError('plugins.json is missing')


MANIFEST = _load()
PLUGINS = {p['id']: p for p in MANIFEST['plugins']}


def setting(cfg, pointer, default=None):
    """A settings.json value by its pointer ('/leaderboard/submit')."""
    at = cfg
    for key in pointer.strip('/').split('/'):
        if not isinstance(at, dict) or key not in at: return default
        at = at[key]
    return at


def switched_on(cfg, pid, meter_on=True):
    """The plug-in's own switch: 'meter' is the DPS meter's (the app's; the meter process sees it as
    wc.shared.switched_off), the others a setting."""
    p = PLUGINS[pid]
    if p['switch'] == 'meter': return bool(meter_on)
    return bool(setting(cfg, p['switch'], p['default']))


def notice_owners(cfg, meter_on=True):
    """{notice kind: shown?}: a kind shows while the plug-in that owns it is on and running and its own notice setting
    (if any) is on. The core's (the button's) only follow their setting."""
    out = {kind: setting(cfg, ptr, True) is not False for kind, ptr in MANIFEST['core']['notices'].items()}
    for pid, p in PLUGINS.items():
        on = switched_on(cfg, pid, meter_on) and not HEALTH.failed(pid)
        for kind, ptr in p['notices'].items():
            out[kind] = on and (ptr is None or setting(cfg, ptr, True) is not False)
    return out


def meter_mode(meter_off, cfg):
    """What the meter process does, from each plug-in's own switch: 'on' (the DPS meter is on: the game is read and the
    meter shows), 'quiet' (the DPS meter is off but another plug-in that needs the game read is on - the leaderboards:
    the game is read, nothing of the meter shows) or 'rest' (none is: nothing is read; the notices still show)."""
    if not meter_off and not HEALTH.failed('dps'): return 'on'
    readers = [pid for pid, p in PLUGINS.items() if pid != 'dps' and 'reader' in p['needs']]
    return 'quiet' if any(switched_on(cfg, pid) and not HEALTH.failed(pid) for pid in readers) else 'rest'


FAILS = 3         # errors in a plug-in's code within FAIL_WINDOW s that stop it
FAIL_WINDOW = 60


class Health:
    """Each plug-in's errors and whether it was stopped for them ({id: why}); the status says so (write_status)."""
    def __init__(self):
        self.errors = collections.defaultdict(collections.deque)
        self.stopped = {}
        self.last = {}

    def failed(self, pid): return pid in self.stopped

    def error(self, pid, exc, where):
        now = time.time(); q = self.errors[pid]
        q.append(now)
        while q and now - q[0] > FAIL_WINDOW: q.popleft()
        self.last[pid] = f'{where}: {exc!r}'[:200]
        logging.getLogger('meter.' + PLUGINS.get(pid, {}).get('log', pid)).exception(f'error in {where} (the other plug-ins go on)')
        if len(q) >= FAILS and pid not in self.stopped:
            self.stopped[pid] = self.last[pid]
            logging.getLogger('meter.' + PLUGINS.get(pid, {}).get('log', pid)).error(
                f'{len(q)} errors in {FAIL_WINDOW} s: stopped until its switch is turned off and on again (or aLogs restarts)')

    def reset(self, pid):
        """Its switch was turned off and on again: it gets another go."""
        if pid in self.stopped or self.errors.get(pid):
            log.info(f'plug-in {pid}: switched off and on again, running again')
        self.stopped.pop(pid, None); self.errors.pop(pid, None)

    def status(self, pid):
        return {'failed': self.stopped.get(pid), 'last_error': self.last.get(pid)}


HEALTH = Health()


def guarded(pid, fallback=None):
    """A plug-in's method called from the core: an error in it is the plug-in's (logged, counted), not the reading
    loop's, and a stopped plug-in's methods do nothing (they give `fallback`)."""
    def wrap(fn):
        @functools.wraps(fn)
        def run(*a, **k):
            if HEALTH.failed(pid): return fallback
            try: return fn(*a, **k)
            except Exception as e:
                HEALTH.error(pid, e, fn.__name__)
                return fallback
        run.guarded = pid
        return run
    return wrap


class SwitchWatch:
    """Notices a plug-in's switch turned off and on again (in the settings the meter reloads), to give a stopped one
    another go."""
    def __init__(self): self.was = {}

    def tick(self, cfg, meter_on):
        for pid in PLUGINS:
            on = switched_on(cfg, pid, meter_on)
            if on and self.was.get(pid) is False: HEALTH.reset(pid)
            self.was[pid] = on
