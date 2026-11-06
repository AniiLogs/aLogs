"""Discord's in-game overlay is noticed once: it draws inside the game's picture, so it's in every proof picture.

    python tests/discord_overlay.py

Meter.overlay_check: with DiscordHook64.dll in the game's process, one log line and one notice (a dict, so it's never
folded into "N records sent"); never again that run; nothing without it or without a game window.
Stand-ins only (the module list is replaced: no game needed). Exit code 1 on any failure.
"""
import os, sys, tempfile
HERE = os.environ.get('ALOGS_BOSSBAR') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
os.environ['ALOGS_DATA'] = tempfile.mkdtemp(prefix='alogs_discord_')
import live, wincapture as wc

fails = []
def check(what, got, want):
    if got != want: fails.append(f'{what}: got {got!r}, want {want!r}')

real = wc.window_modules
def meter(mods, hwnd=123):
    m = live.Meter.__new__(live.Meter); m.hwnd = hwnd; m.sent = []
    m.out = lambda kind, data: m.sent.append((kind, data))
    wc.window_modules = lambda h: set(mods)
    return m
try:
    m = meter({'unityplayer.dll', 'discordhook64.dll'})
    m.overlay_check(); m.overlay_check()
    check('one notice', len(m.sent), 1)
    check('a toast', m.sent[0][0] if m.sent else None, 'toast')
    check('a notice, not a record', isinstance(m.sent[0][1], dict) and m.sent[0][1].get('kind'), 'notice')
    m = meter({'unityplayer.dll', 'gameoverlayrenderer64.dll'})
    m.overlay_check(); check('no Discord, no notice', m.sent, [])
    m = meter({'discordhook64.dll'}, hwnd=None)
    m.overlay_check(); check('no game window, no notice', m.sent, [])
finally: wc.window_modules = real
check('the real module list is a set', isinstance(wc.window_modules(0), set), True)

if fails:
    print('\n'.join(fails)); sys.exit(1)
print('discord overlay: ok')
