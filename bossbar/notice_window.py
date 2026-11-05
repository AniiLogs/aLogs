"""aLogs' notices over the game: every plug-in's (the leaderboards' records, Sanctum Helper's, the button's), shown by
the meter process for all of them - part of the core, not of any one plug-in. run_overlay (live.py) makes the window;
each plug-in hands its notices over (records: the overlay's queue; other programs: a JSON file in NOTICES)."""
from core import *


def notice_settings(cfg):
    """General Settings > Notifications: where notices show, how they come in, and whether the custom spot is locked."""
    n = cfg.get('notifications') or {}
    return {'place': n.get('place', 'top-right'), 'motion': n.get('motion', 'fade'), 'locked': n.get('locked', True) is not False}


def notice_window(tk, root, meter, hide, AWAY, present, park, pos, save_positions, OFF, rgb):
    """The notice window and its queue: returns (window, state, tick); tick() runs with the overlay's own (15 ms)."""
    import layered
    # Every plug-in's notices over the game, one at a time, in one style (a dark card with a coloured bar, its title
    # over the details), only while the game is in front (they wait otherwise): the leaderboards' "Interlink run sent"
    # (records.sent_notice, Sarg's suggestion, 3 Nov), Sanctum Helper's ("Chaos Mode found.") and the button's ("aLogs
    # off"). Each plug-in owns its notices: one switched off drops its own. Where they show and how they come in are
    # the player's (General Settings > Notifications: notifications.place, motion). Three or more records waiting at
    # once (a backlog sent after being offline) make one notice with the count. Other programs drop each notice as a
    # small JSON file in NOTICES ({head, detail, accent, from, screen: the game window's client area, which the meter
    # may not know in a homeland}); read here, then deleted. 'custom': the player's own spot - unlocked
    # (notifications.locked off), a sample card stays up while the game is in front, to drag there (double-click: back
    # to the top right); saved as a share of the game's height from its top-left corner, like the DPS window's spot.
    tw = tk.Toplevel(root); tw.overrideredirect(True); tw.attributes('-topmost', True); tw.geometry(OFF)
    toast = {'queue': [], 'since': None, 'alpha': -1, 'sample': None, 'drag': None}
    TOAST_SHOW, TOAST_IN, TOAST_OUT = 4.0, 0.25, 0.35
    TOAST_W, TOAST_H, TOAST_MARGIN = 360, 64, 18
    def toast_font(px, bold):
        for face in (('seguisb.ttf', 'segoeui.ttf') if bold else ('segoeui.ttf',)):
            try: return ImageFont.truetype(face, px)
            except OSError: pass
        return ImageFont.load_default()
    def toast_image(head, detail, accent, s):
        """The card at scale s (1 at 1080p): 360x64, 12 px corners, a 5 px bar in the accent colour, the title in
        white semibold over the details in grey (Sanctum Helper's ToastWindow), each cut short with ... if too long."""
        W, H, r = round(TOAST_W * s), round(TOAST_H * s), round(12 * s)
        img = Image.new('RGBA', (W, H), (0, 0, 0, 0)); d = ImageDraw.Draw(img)
        d.rounded_rectangle((0, 0, W - 1, H - 1), r, fill=(20, 22, 31, 235), outline=(255, 255, 255, 70), width=max(1, round(s)))
        bx, bw = round(14 * s), max(3, round(5 * s))
        d.rounded_rectangle((bx, round(10 * s), bx + bw, H - round(10 * s)), bw // 2 + 1, fill=rgb(accent) + (255,))
        tx = bx + bw + round(12 * s); room = W - tx - round(16 * s)
        def fit(text, font):
            if d.textlength(text, font=font) <= room: return text
            while text and d.textlength(text + '…', font=font) > room: text = text[:-1]
            return text.rstrip(' ·') + '…'
        big, small = toast_font(max(11, round(16 * s)), True), toast_font(max(10, round(14 * s)), False)
        if detail:
            d.text((tx, H * 0.36), fit(head, big), font=big, fill=(255, 255, 255, 255), anchor='lm')
            d.text((tx, H * 0.68), fit(detail, small), font=small, fill=(196, 202, 216, 255), anchor='lm')
        else: d.text((tx, H / 2), fit(head, big), font=big, fill=(255, 255, 255, 255), anchor='lm')
        return img
    def toast_spot(place, ox, oy, W, H, w, h, s):
        """(x, y, the edge it slides from) of a notice of w x h on the game screen (ox, oy, W, H) at scale s."""
        m = round(TOAST_MARGIN * s)
        spot = pos.get('notice')
        if place == 'custom' and isinstance(spot, list) and len(spot) == 2:
            x = min(max(ox, ox + spot[0] * H), ox + W - w); y = min(max(oy, oy + spot[1] * H), oy + H - h)
            return x, y, 'left' if x + w / 2 < ox + W / 2 else 'right'
        if place == 'top-center': return ox + (W - w) / 2, oy + round(150 * s), 'top'     # under a boss's banner
        if place == 'right': return ox + W - w - m, oy + H * 0.42, 'right'
        if place == 'left': return ox + m, oy + H * 0.42, 'left'
        return ox + W - w - m, oy + round((TOAST_MARGIN + 64 + 10) * s), 'right'    # under Sanctum Helper's notices
    def toast_screen():
        screen = meter.last_screen or (0, 0, root.winfo_screenwidth(), root.winfo_screenheight())
        return screen, min(screen[2], screen[3] * 16 / 9) * 9 / 16 / 1080
    def toast_grab(e):
        if toast['sample']: toast['drag'] = (e.x_root - toast['sample'][0], e.y_root - toast['sample'][1])
    def toast_move(e):
        if not toast['drag'] or not toast['sample']: return
        x, y = e.x_root - toast['drag'][0], e.y_root - toast['drag'][1]
        toast['sample'][:2] = [x, y]; present(tw, toast['sample'][2], x, y)
    def toast_drop(e):
        if not toast['drag'] or not toast['sample']: return
        toast['drag'] = None; (ox, oy, W, H), s = toast_screen()
        pos['notice'] = [(toast['sample'][0] - ox) / H, (toast['sample'][1] - oy) / H]; save_positions()
        toast['sample'] = None                                                  # drawn again where it was dropped
    def toast_reset(e):
        pos.pop('notice', None); save_positions(); toast['drag'] = None; toast['sample'] = None
    for ev, fn in (('<Button-1>', toast_grab), ('<B1-Motion>', toast_move), ('<ButtonRelease-1>', toast_drop),
                   ('<Double-Button-1>', toast_reset)):
        tw.bind(ev, fn)
    def toast_sample():
        """Custom and unlocked: the sample card to drag, up while the game is in front and no notice is showing."""
        nc = notice_settings(meter.args.cfg)
        want = (nc['place'] == 'custom' and not nc['locked']
                and toast['since'] is None and not AWAY['on'] and not hide)
        if not want:
            if toast['sample'] and not toast['drag']: park(tw); layered.park(layered.top_hwnd(tw)); toast['sample'] = None
            return
        if toast['sample'] or toast['drag']: return
        (ox, oy, W, H), s = toast_screen()
        img = toast_image('Drag me where aLogs notices should show', 'Lock it in General Settings \u203a Notifications. Double-click: top right',
                          '#c4cad8', s)
        x, y, _ = toast_spot('custom' if pos.get('notice') else 'top-right', ox, oy, W, H, img.width, img.height, s)
        toast['sample'] = [round(x), round(y), img]; toast['alpha'] = -1
        present(tw, img, round(x), round(y)); layered.click_through(layered.top_hwnd(tw), False)
        layered.set_alpha(layered.top_hwnd(tw), 255)
    def take_notices(now):
        """Sanctum Helper's notice files, four times a second: into the queue in the order they came, then deleted."""
        if now - toast.get('looked', 0) < 0.25: return
        toast['looked'] = now
        try: names = sorted(n for n in os.listdir(NOTICES) if n.endswith('.json'))
        except OSError: return
        for name in names:
            path = os.path.join(NOTICES, name)
            try:
                with open(path, encoding='utf-8') as f: d = json.load(f)
                screen = d.get('screen')
                screen = tuple(int(v) for v in screen) if isinstance(screen, list) and len(screen) == 4 and screen[2] > 0 and screen[3] > 0 else None
                accent = d.get('accent') if re.fullmatch(r'#[0-9a-fA-F]{6}', str(d.get('accent'))) else '#5cb8ff'
                if d.get('head'): toast['queue'].append({'head': str(d['head'])[:120], 'detail': str(d.get('detail') or '')[:120],
                                                         'accent': accent, 'screen': screen, 'kind': str(d.get('from') or 'notice')})
            except (OSError, ValueError, TypeError): pass
            try: os.remove(path)
            except OSError: pass
    def toast_tick():
        now = time.time()
        take_notices(now)
        # (records come as (head, detail, accent) from submit.sent_notice)
        toast['queue'] = [n if isinstance(n, dict) else {'head': n[0], 'detail': n[1], 'accent': n[2], 'screen': None, 'kind': 'record'}
                          for n in toast['queue']]
        # each plug-in owns its notices: one switched off since they came drops them (records: the leaderboards;
        # 'sanctum': Sanctum Helper); this window shows them for every plug-in
        cfg = meter.args.cfg
        lb = cfg.get('leaderboard') or {}
        owner_on = {'record': bool(lb.get('submit')) and lb.get('notify_sent', True) is not False,
                    'sanctum': (cfg.get('sanctum') or {}).get('enabled', True) is not False,
                    'button': (cfg.get('notifications') or {}).get('button', True) is not False}
        toast['queue'] = [n for n in toast['queue'] if owner_on.get(n['kind'], True)]
        toast_sample()
        if toast['sample'] or toast['drag']: return                             # placing it: no notices until locked
        if toast['since'] is None:
            if not toast['queue'] or AWAY['on'] or hide: return
            first = toast['queue'][0]
            records = 0
            while records < len(toast['queue']) and toast['queue'][records]['kind'] == 'record': records += 1
            if records >= 3:                                                   # a backlog of records: one notice
                head, detail, accent = f'{records} records sent to the leaderboards', '', '#7fb2ff'
                del toast['queue'][:records]
            else: head, detail, accent = first['head'], first['detail'], first['accent']; del toast['queue'][0]
            if not meter.last_screen and first.get('screen'):                  # (in a homeland the meter may not know it)
                screen = first['screen']; s = min(screen[2], screen[3] * 16 / 9) * 9 / 16 / 1080
            else: screen, s = toast_screen()
            ox, oy, W, H = screen
            img = toast_image(head, detail, accent, s)
            nc = notice_settings(meter.args.cfg)
            x, y, edge = toast_spot(nc['place'], ox, oy, W, H, img.width, img.height, s)
            toast.update(since=now, img=img, at=(x, y), edge=edge, motion=nc['motion'],
                         screen=screen, alpha=-1, shown=None)
            log.info(f"notice: {head}{' - ' + detail if detail else ''}")
        t = now - toast['since']
        if t >= TOAST_SHOW:
            park(tw); layered.park(layered.top_hwnd(tw)); toast['since'] = None; return
        e = min(1.0, t / TOAST_IN, (TOAST_SHOW - t) / TOAST_OUT)
        e = 1 - (1 - e) ** 3                                                   # eases out coming in, in going out
        x, y = toast['at']; img = toast['img']; ox, oy, W, H = toast['screen']
        if toast['motion'] == 'slide':                                        # in from the nearest edge, back out
            gap = {'right': ox + W - x, 'left': x - ox + img.width, 'top': y - oy + img.height}[toast['edge']]
            if toast['edge'] == 'right': x += (1 - e) * gap
            elif toast['edge'] == 'left': x -= (1 - e) * gap
            else: y -= (1 - e) * gap
        elif toast['motion'] == 'drop': y -= (1 - e) * (y - oy + img.height)   # down from the top, back up
        spot = (round(x), round(y))
        if spot != toast['shown'] and not AWAY['on']:
            present(tw, img, *spot); layered.click_through(layered.top_hwnd(tw)); toast['shown'] = spot; toast['alpha'] = -1
        a = round(255 * e)
        if a != toast['alpha'] and not AWAY['on']: layered.set_alpha(layered.top_hwnd(tw), a); toast['alpha'] = a

    return tw, toast, toast_tick
