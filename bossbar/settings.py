"""Where the meter finds its files, and the settings the desktop app edits.

RES   read-only files shipped with the meter (boss catalog, digit templates, OCR helper). Inside the packaged build
      this is PyInstaller's bundle folder.
DATA  everything the meter writes: logs, records, calibration, overlay positions, submission queue, settings.json.
      Packaged: %LOCALAPPDATA%\\<APP_DIR>\\meter. From source: next to the code. ALOGS_DATA overrides both.

settings.json is shared with the desktop app, which writes it from its menus; missing keys fall back to DEFAULTS.
"""
import copy, json, os, sys

APP_DIR = 'aLogs'          # the suite's short name (aLogs: Aniiloger's Companion App); the data folder's name
FROZEN = getattr(sys, 'frozen', False)
RES = getattr(sys, '_MEIPASS', os.path.dirname(os.path.abspath(__file__)))
DATA = os.environ.get('ALOGS_DATA') or (
    os.path.join(os.environ.get('LOCALAPPDATA', os.path.expanduser('~')), APP_DIR, 'meter') if FROZEN else RES)
os.makedirs(DATA, exist_ok=True)
SETTINGS_FILE = os.path.join(DATA, 'settings.json')
STATUS_FILE = os.path.join(DATA, 'status.json')

DEFAULTS = {
    'meter': {'enabled': True},               # the desktop app starts the meter only while this is on
    'audio_cues': {                          # the game's own sounds, heard through the app (Aniimo's audio only)
        'boss_kill': True,                   # the boss kill sound dates the killing blow out in the world
    },
    'app': {                                 # used by the desktop app, kept here so both sides agree
        'welcome_done': False,
        'show_guide': True,                  # the setup guide when aLogs starts (until finished or skipped)
        'start_with_windows': False,
        'close_to_tray': True,
        'priority': '',                      # how much of a busy PC aLogs may take: '' (not set: Windows decides), low, normal, high
        'auto_download': True,               # the app downloads updates in the background (About › Updates)
        'beta_updates': False,               # the app's self-update follows the beta feed too (About › Updates)
        'photos_auto': False,                # Photos: upload new in-game photos to the profile on their own
        'photos_auto_since': 0,              # ...taken after this (unix time: when that was switched on)
    },
    'sanctum': {                             # Sanctum Helper module (its own options live in its settings.json)
        'enabled': True,
    },
    'game': {
        'window_title': 'Aniimo',
        'player_log': '',                    # empty = the game's default Player.log location
    },
    'overlay': {
        'show': True,                        # any overlay at all (off = console/app only)
        'dps_window': True,
        'hp_labels': True,
        'in_recordings': True,               # overlays show in recordings and screenshots (General Settings; shared
                                             # with Sanctum Helper). Replaced hide_from_capture, its opposite.
        'backdrop': 0,                       # dark panel behind the DPS window, % opacity (0 = none)
        'world_position': 'map',             # out in the world: 'map' (right of the minimap) or 'right' (top right)
        'dps_locked': True,                  # the DPS window can't be dragged, and clicks go through it to the game
        'dps_reset': 0,                      # the app bumps this to put the DPS window back in its default spots
        'rush_position': 'corner',           # Interlink's DPS window: 'corner' (top left, by Esc) or 'panel' (under the stars/timer)
        'result_seconds': 10,                # out in the world: how long the fight's result stays up (0 = hide at once)
    },
    'capture': {
        'look_interval': 0.05,               # seconds between looks between fights
        'damage_numbers': False,             # experimental: read your own damage numbers (now: record fights)
        'test_frames': False,                # full-size pictures for the accuracy tests (Captures & logs)
        'fight_rate': 30,                    # frames a second during a fight: 30 (default), or 60 for the finest timing
        'debug_frames': False,               # bar crops per fight in logs/<fight>/ (diagnostics)
        'end_screens': False,                # frames after each fight in logs/end_screens/ (diagnostics)
        'nobar_frames': False,               # frames when no bar is found in a dungeon (diagnostics)
        # pictures: 0 = only until their data is saved (and, as leaderboard proof, sent); N = N hours; -1 = keep all
        'keep_hours': 0,
    },
    # The automation that collects leaderboard proof (Leaderboards › Automation): only with leaderboard.submit and
    # leaderboard.automation on (automation(), below); each of these is one thing it may do.
    'results': {
        'auto_open_battle_data': True,       # Interlink: click "View Battle Data" on the result screen
        'sim_gizmos': True,                  # Holo-Battle Sim: open the Gizmo screen after a win and read it
        'sim_gate': False,                   # Holo-Battle Sim: press End Holo-Battle Sim at the exit gate, then the above
        'hold_inputs': True,                 # hold keyboard keys/mouse buttons while doing so
        'hold_controller': False,            # also hide the controller (needs HidHide)
        'egg_fade_hold': False,              # Egg Heist: hold inputs from the fade to black until the end screen is read (experimental)
        'hold_seconds': 60,                  # the longest aLogs keeps control, each time
    },
    'leaderboard': {
        'submit': False,                     # opt-in: send records to the AniiLogs leaderboards
        'automation': False,                 # opt-in: let aLogs take control of the game to collect proof (results.*)
        'api_url': 'https://aniilogs-api.pages.dev/api',
        'token': '',                         # desktop token from the leaderboard page
        'link_character': True,              # link the detected character (UID + name) to the account
        'include_screenshots': True,         # attach the result screenshot as evidence
    },
    # the settings the player changed themselves ('capture.keep_hours', ...), kept by the app: a default that changes in
    # a later version only moves the ones not listed, so a choice is never overwritten
    'user_set': [],
}

def _merge(base, extra):
    out = copy.deepcopy(base)
    for key, value in (extra or {}).items():
        if isinstance(out.get(key), dict) and isinstance(value, dict): out[key] = _merge(out[key], value)
        elif key in out: out[key] = value
    return out

def automation(cfg, what=None):
    """May the meter take control of the game for leaderboard proof (and do `what`, a results.* switch)? Only with
    sending and the automation both on: it collects proof for the leaderboards, and is opt-in."""
    lb = cfg.get('leaderboard') or {}
    if not (lb.get('submit') and lb.get('automation')): return False
    return what is None or bool((cfg.get('results') or {}).get(what))

def load():
    try:
        with open(SETTINGS_FILE, encoding='utf-8') as f: stored = json.load(f)
    except (OSError, ValueError): stored = {}
    overlay = stored.get('overlay') if isinstance(stored.get('overlay'), dict) else {}
    if 'in_recordings' not in overlay and 'hide_from_capture' in overlay:      # settings from before 0.1.28
        overlay['in_recordings'] = not overlay['hide_from_capture']
    if overlay.get('world_position') == 'quests': overlay['world_position'] = 'right'   # B9: under the quests is gone
    return _merge(DEFAULTS, stored)

def save(settings):
    tmp = SETTINGS_FILE + '.tmp'
    with open(tmp, 'w', encoding='utf-8') as f: json.dump(_merge(DEFAULTS, settings), f, indent=1)
    os.replace(tmp, SETTINGS_FILE)

def write_status(status):
    """status.json for the desktop app's dashboard (written about once a second)."""
    tmp = STATUS_FILE + '.tmp'
    try:
        with open(tmp, 'w', encoding='utf-8') as f: json.dump(status, f)
        os.replace(tmp, STATUS_FILE)
    except OSError: pass
