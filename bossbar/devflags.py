"""Dev switches (Beta Menu > Dev): whether a proposed change is turned on. See docs/plugins.md.

The app keeps the switches (app/src-tauri/src/dev.rs) and writes the ones that are on to a small file, and only on a beta
build does it tell the meter where that file is (ALOGS_DEV_ON). So on a release build, or a meter started on its own,
on() is False without looking at any file, and with every switch off the meter does what it did before.

on() is called from the reading loop: it looks at the file at most once a second, and never raises."""
import json, logging, os, time

log = logging.getLogger('meter')

CHECK_S = 1.0
_path = os.environ.get('ALOGS_DEV_ON') or None
_on = frozenset()
_stamp = None
_next = 0.0

def on(option):
    """Is this switch on? False on anything unexpected."""
    global _next
    if _path is None: return False
    now = time.monotonic()
    if now >= _next:
        _next = now + CHECK_S
        _look()
    return option in _on

def _look():
    """The file, if it changed. Gone or unreadable: all off."""
    global _on, _stamp
    try:
        st = os.stat(_path)
        stamp = (st.st_mtime_ns, st.st_size)
        if stamp == _stamp: return
        with open(_path, encoding='utf-8') as f: found = frozenset(x for x in json.load(f)['on'] if isinstance(x, str))
        _stamp = stamp
    except Exception:
        found = frozenset(); _stamp = None
    if found != _on:
        _on = found
        try: log.info(f"dev switches on: {', '.join(sorted(found)) or 'none'}")     # so a report says what was on
        except Exception: pass

def use(path):
    """Tests: read this file from now on (None: as on a release build)."""
    global _path, _on, _stamp, _next
    _path, _on, _stamp, _next = path, frozenset(), None, 0.0
