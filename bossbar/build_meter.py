"""Packages the meter as a standalone program for the desktop app's installer.

    python build_meter.py

Builds dist/meter/meter.exe with PyInstaller (one folder, no console window; nothing needs installing on the
player's PC) and copies it, plus HidHide's signed driver files and license (vendor/hidhide), into
../app/src-tauri/resources/, where the app's installer picks them up.
"""
import os, shutil, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
APP_RES = os.path.normpath(os.path.join(HERE, '..', 'app', 'src-tauri', 'resources'))
DATA_FILES = ['boss_catalog.csv', 'boss_rush_grades.csv', 'boss_rush_cycles.json', 'xn_templates.json', 'boss_break.json', 'ocr_win.ps1',
              'ocr-helper.exe']   # built by the app's `npm run ocr:build`

def main():
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
