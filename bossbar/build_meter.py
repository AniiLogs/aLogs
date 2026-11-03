"""Packages the meter as a standalone program for the desktop app's installer.

    python build_meter.py

First checks the shipped boss list and grade table against the installed game (check_data: a difference
stops the build). Then builds dist/meter/meter.exe with PyInstaller (one folder, no console window; nothing needs
installing on the player's PC) and copies it, plus HidHide's signed driver files and license (vendor/hidhide), into
../app/src-tauri/resources/, where the app's installer picks them up.
"""
import os, shutil, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
APP_RES = os.path.normpath(os.path.join(HERE, '..', 'app', 'src-tauri', 'resources'))
DATA_FILES = ['boss_catalog.csv', 'boss_rush_grades.csv', 'boss_rush_cycles.json', 'xn_templates.json', 'boss_break.json', 'ocr_win.ps1',
              'ocr-helper.exe']   # built by the app's `npm run ocr:build`

def check_data():
    """Stops the build when the shipped boss list or Interlink HP-bar table isn't the game's (gamedata.catalog_drift,
    gamedata.grades_drift). A stale boss list (no in_boss_rush on the second bosses of weeks 4003-4005's middle fights)
    matched Alpha Jabster to the left boss Alpha Tubster and found no boss for Omega Tucking and Omega Jeburst; a stale
    grade table (16 bars) put every Interlink fight's damage on the wrong bar."""
    sys.path.insert(0, HERE)
    import gamedata
    game = gamedata.find_game_data()
    where = game or 'what can be checked without the game (no game install found)'
    stop = False
    for name, fix, drift in (('boss_catalog.csv', 'catalog', gamedata.catalog_drift(game)),
                             ('boss_rush_grades.csv', 'grades', gamedata.grades_drift(game))):
        if drift:
            print(f'{name} differs from {where}:')
            for line in drift[:40]: print('  ' + line)
            print(f'  fix: `python gamedata.py {fix}` (and read the diff), or update the game first'); stop = True
        else: print(f'{name} checked against {where}')
    if stop: sys.exit('build stopped: the shipped game data is out of date')

def main():
    check_data()
    work = os.path.join(HERE, 'build')
    dist = os.path.join(HERE, 'dist')
    args = [sys.executable, '-m', 'PyInstaller', '--noconfirm', '--clean', '--onedir', '--windowed',
            '--name', 'meter', '--distpath', dist, '--workpath', work, '--specpath', work,
            # only what the meter uses; keeps the bundle small
            '--exclude-module', 'numpy', '--exclude-module', 'matplotlib', '--exclude-module', 'PyQt5',
            '--exclude-module', 'IPython', '--exclude-module', 'pytest',
            # reads the game's own HUD font from the player's install the first time (gamefont.py)
            # (plus two of its own dependencies' data files, which it loads on import)
            '--collect-all', 'UnityPy', '--collect-all', 'fmod_toolkit', '--collect-all', 'archspec']
    for name in DATA_FILES:
        args += ['--add-data', f'{os.path.join(HERE, name)}{os.pathsep}.']
    # the Korean name model built into ocr-helper.exe: its licence (Apache-2.0) goes with it
    args += ['--add-data', f'{os.path.join(HERE, "..", "app", "ocr-helper", "model", "LICENSE-model.txt")}{os.pathsep}.']
    args.append(os.path.join(HERE, 'live.py'))
    subprocess.run(args, check=True, cwd=HERE)

    target = os.path.join(APP_RES, 'meter')
    shutil.rmtree(target, ignore_errors=True)
    shutil.copytree(os.path.join(dist, 'meter'), target)
    vendor = os.path.join(APP_RES, 'vendor')
    shutil.rmtree(vendor, ignore_errors=True)
    shutil.copytree(os.path.join(HERE, 'vendor'), vendor)
    size = sum(os.path.getsize(os.path.join(root, f)) for root, _, files in os.walk(target) for f in files)
    print(f'meter packaged: {target} ({size / 1e6:.1f} MB)')

if __name__ == '__main__':
    main()
