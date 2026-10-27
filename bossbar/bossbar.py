"""Aniimo boss-bar reader for screenshots.

Reads level, boss title, segment counter (xN), HP-bar fill and the countdown timer, converts the fill into team
damage using the decoded boss HP formulas, and reports DPS. Works at any resolution (see reader.py).

Usage:
    python bossbar.py shot1.png [shot2.png ...] [--boss-id ID] [--level N] [--cropped] [--duration 180] [--not-bossrush]

Only reads image files. Text is read with Windows' built-in OCR (ocr_win.ps1).
"""
import argparse, json, os
from PIL import Image
import reader as R

def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('shots', nargs='+'); ap.add_argument('--boss-id'); ap.add_argument('--level', type=int)
    ap.add_argument('--cropped', action='store_true', help='image is a crop around the boss bar (no timer)')
    ap.add_argument('--duration', type=int, default=180, help='fight length in seconds (boss rush: 180)')
    ap.add_argument('--not-bossrush', action='store_true', help='plain bar: fill = share of total HP')
    args = ap.parse_args()
    catalog, cum, ocr = R.load_catalog(), R.load_grades(), R.Ocr(); results = []
    try:
        for p in args.shots:
            f = R.read_frame(Image.open(p), ocr, cropped=args.cropped)
            r = {'file': os.path.basename(p)}
            if not f: r['error'] = 'boss bar not found'; print(json.dumps(r)); continue
            b = f['bar']
            r.update({'size': Image.open(p).size, 'bar_span_px': round(b['span']), 'brackets_found': b['brackets_found'],
                      'fill_pct': round(f['fill'] * 100, 2), 'flash_px': f['flash_px'], 'ghost_px': f['ghost_px'],
                      'ocr': f['ocr'], 'level': f.get('level'), 'title': f.get('title'), 'xn': f.get('xn'), 'timer_s': f.get('timer_s')})
            level = args.level or r['level']; bossrush = not args.not_bossrush and r['xn'] is not None
            row, nvar = R.pick_boss(catalog, r['title'], bossrush, args.boss_id)
            r['boss'] = row and {'puppetId': row['puppetId'], 'title': row['bossTitle_en'], 'variants': nvar}
            if row and level:
                hp = R.boss_max_hp(row, level); r['max_hp'] = hp
                dmg, seg = R.damage_dealt(hp, f['fill'], r['xn'], cum, bossrush)
                r['segment'] = seg; r['team_damage'] = dmg
                if dmg is not None and r['timer_s'] is not None:
                    el = args.duration - r['timer_s']; r['elapsed_s'] = el
                    r['team_dps'] = round(dmg / el) if el > 0 else None
            results.append(r); print(json.dumps(r, ensure_ascii=False))
    finally:
        ocr.close()
    ok = [r for r in results if r.get('team_damage') is not None and r.get('elapsed_s') is not None]
    for a, b in zip(ok, ok[1:]):
        dt = b['elapsed_s'] - a['elapsed_s']
        if dt > 0:
            d = b['team_damage'] - a['team_damage']
            print(f"interval {a['file']} -> {b['file']}: {d:,} dmg in {dt}s = {d / dt:,.0f} DPS")

if __name__ == '__main__':
    main()
