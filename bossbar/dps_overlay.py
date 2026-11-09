"""The DPS meter plug-in: what it shows over the game - the DPS window, the HP labels on the boss bar, the notes on
end screens, the "You're being attacked" alert - from the fights the core follows (fights.py). Its own switch:
"Run the DPS meter" (the app's; the meter sees it as wc.shared.switched_off). The window loop also runs the
core's notices (notice_window.py), so they show whatever this plug-in's switch."""
from core import *
from notice_window import notice_window
import logging

log = logging.getLogger('meter.dps')   # its lines in meter.log are tagged [dps]

def offset_on_screen(off, x, y, ox, oy, W, H):
    """Whether a saved offset (a fraction of the game's height) from the spot (x, y) still lands in the game's area
    (ox, oy, W, H)."""
    nx, ny = x + off[0] * H, y + off[1] * H
    return ox - H * 0.2 <= nx <= ox + W and oy - H * 0.2 <= ny <= oy + H

def run_overlay(meter, hide):
    """The DPS window (drag to move, right-click to close the meter) and three HP labels attached to the boss bar.
    Positions are remembered in overlay_positions.json."""
    import tkinter as tk, tkinter.font as tkfont
    from PIL import ImageTk
    import gamefont, layered
    BG, FG, DIM = '#14181f', '#e8e6e3', '#a9b1bd'
    show_dps_window = meter.args.cfg['overlay']['dps_window']
    show_hp_labels = meter.args.cfg['overlay']['hp_labels']
    # a dark panel behind the DPS window for readability over bright scenes: 0 (none) to 100 (solid), in %
    BACKDROP = max(0, min(100, int(meter.args.cfg['overlay'].get('backdrop', 0) or 0)))
    q = queue.Queue(); meter.out = lambda k, d: (q.put((k, d)), console_out(k, d))
    meter.takes_notices = True                        # status.json: Sanctum Helper's notices come here (NOTICES)
    pos = load_positions()
    root = tk.Tk(); root.withdraw()
    def save_positions():
        try:
            S.write_json(SETTINGS, pos)
        except OSError: pass
    def quit_all(e=None): save_positions(); root.destroy()
    OFF = '+-32000+-32000'                            # hidden = parked off screen (showing again never takes focus)
    KEY = '#0b0e14'                                   # made transparent; close to the shadow so edges blend
    families = set(tkfont.families(root))
    FACE = next((f for f in ('Bahnschrift SemiBold SemiConden', 'Bahnschrift SemiBold', 'Segoe UI Semibold') if f in families),
                'Segoe UI Semibold')                  # narrow, rounded numerals like the game's HUD font
    # The game's own HUD font, read once from the game install into the data folder (gamefont.py). Loading takes a
    # moment the first time, so it happens in the background; until then (or without it) Bahnschrift stands in.
    GF = {'font': None}
    def load_font():
        GF['font'] = gamefont.GameFont.load(os.path.join(HERE, 'gamefont'), getattr(meter.args, 'player_log', None))
        log.info('overlay font: ' + ('the game\'s own (UI_Font_Europe)' if GF['font']
                                     else f'{FACE} (game font not readable: {gamefont.GameFont.error})'))
        if gamefont.GameFont.trace: log.info(gamefont.GameFont.trace)
    threading.Thread(target=load_font, daemon=True).start()
    KEY_RGB = tuple(int(KEY[i:i + 2], 16) for i in (1, 3, 5))
    rgb = lambda colour: tuple(int(colour[i:i + 2], 16) for i in (1, 3, 5))

    # Over the game only: while another window is in front (you tabbed out), every overlay window stays parked off
    # screen; each remembers where it belongs and comes back when the game is in front again.
    AWAY = {'on': False}; placed = {}
    # Where the DPS window is drawn (screen pixels), for the meter to black out of its pictures of the screen
    # (Meter.mask_own): with that, the DPS window and its notes can stay in screenshots and recordings on Windows 10
    # too. A spot it just left is kept a moment: the picture being read can be a little older than the move.
    meter.own_rects = []; meter.own_in_capture = None       # None: not set yet (poll_tick sets it either way)
    def overlay_now():
        """[(RGBA image, screen x, screen y)] of the DPS window while it's shown (Meter.proof_with_overlay)."""
        p = placed.get(dw)
        return [(p[1], p[2], p[3])] if p and p[0] == 'img' and dps_at.get('shown') else []
    meter.overlay_now = overlay_now
    own = {'cur': {}, 'old': []}
    def own_spot(win, rect):
        """Where each overlay window is (the DPS window, its notes, the HP labels), for Meter.mask_own; a spot just left
        is kept half a second, as the picture being read can be a little older than the move."""
        now = time.time(); prev = own['cur'].get(win)
        if prev == rect: return
        if prev: own['old'].append((*prev, now + 0.5))
        if rect: own['cur'][win] = rect
        else: own['cur'].pop(win, None)
        own['old'] = [r for r in own['old'] if r[4] > now]
        meter.own_rects = [(*r, 0) for r in own['cur'].values()] + own['old']
        publish_own(now)
    def publish_own(now):
        """Our spots for Sanctum Helper and the app to cover in their pictures (and again each second: a stale block
        is taken for a closed meter)."""
        own['published'] = now
        try: wc.shared.publish_overlays(wc.shared.OWNER_METER, [r[:4] for r in meter.own_rects if not r[4] or r[4] > now])
        except Exception: pass
    def present(win, image, x, y):
        """With the game font: the window is this RGBA image, with real per-pixel alpha (layered.py)."""
        placed[win] = ('img', image, x, y)
        own_spot(win, (int(x), int(y), int(x) + image.width, int(y) + image.height))
        if not AWAY['on']: layered.show(layered.top_hwnd(win), image, x, y)
    def place(win, x, y):
        """Without the game font: move the window there (a plain Tk window)."""
        placed[win] = ('geo', x, y)
        win.update_idletasks()
        own_spot(win, (int(x), int(y), int(x) + win.winfo_reqwidth(), int(y) + win.winfo_reqheight()))
        if not AWAY['on']: win.geometry(f'+{int(x)}+{int(y)}')
    def park(win): placed.pop(win, None); own_spot(win, None); win.geometry(OFF)
    def spot(win):
        """Where a window is drawn. Not Tk's winfo_x: a window drawn with `present` was moved by Windows (layered.show), and
        Tk still has it parked at -32000 (8 Nov: dragging the DPS window threw it off screen, and saved that spot)."""
        p = placed.get(win)
        return (int(p[-2]), int(p[-1])) if p else (win.winfo_x(), win.winfo_y())
    def move_to(win, x, y, image=None):
        """A window being dragged follows the mouse, in whichever way it's drawn."""
        if image is not None: present(win, image, x, y)
        else: place(win, x, y)

    def ink(im):
        """(left, right) of the letters' solid part, leaving out the soft shadow (at most ~120 alpha), which spreads
        further at larger sizes. Italic text is measured on its lower half: it leans right above that."""
        a = im.getchannel('A')
        if im.info.get('italic'): a = a.crop((0, a.height // 2, a.width, a.height))
        a = a.point(lambda v: 255 if v > 140 else 0)
        box = a.getbbox()
        return (box[0], box[2]) if box else (0, im.width)

    def compose(lines, H, divider=False, center=False, backdrop=None):
        """The stats as one RGBA image in the game font: lines [(text, colour, size, 'i'/'b')], optional divider.
        A text that is a (label, value) pair is a row: the label grey on the left, the value on the right; with rows,
        every line's right edge lines up on the letters. backdrop: the dark card's strength in % (the Overlay setting
        unless given)."""
        backdrop = BACKDROP if backdrop is None else backdrop
        f = GF['font']; pad = max(3, round(H * (0.011 if backdrop else 0.004))); parts = []
        rows = any(isinstance(line[0], tuple) for line in lines)
        for text, colour, size, *style in lines:
            px = max(11, round(H * size)); style = style[0] if style else ''
            if isinstance(text, tuple):
                parts.append(((f.render(text[0], px, rgb('#aab4c3')), f.render(text[1], px, rgb(colour))), px))
            else: parts.append((f.render(text, px, rgb(colour), italic='i' in style, bold='b' in style), px))
        def span(im): left, right = ink(im); return right - left
        def above(im): return max(p.info.get('base', p.height) for p in im)     # a row: label and value share a baseline
        def tall(im):
            if not isinstance(im, tuple): return im.height
            return above(im) + max(p.height - p.info.get('base', p.height) for p in im)
        gap = round(H * 0.012)
        if rows:          # measured on the ink, so the right edges and the labels' left edges agree to the pixel; at
            #               least as wide as a long reading, so the card doesn't jump as the numbers change
            px0 = max(11, round(H * 0.015))
            least = span(f.render('Last 5s', px0)) + gap + span(f.render('888.8K / 8.88M HP', px0))
            width = max([least] + [span(im[0]) + gap + span(im[1]) if isinstance(im, tuple) else span(im)
                                   for im, _ in parts])
        else: width = max(im.width for im, _ in parts)
        width = max([width] + ([round(H * 0.215)] if divider else []))
        lw = max(1, round(H / 1080)); top = round(H * 0.01) if divider else 0
        height = top + sum(tall(im) + round(px * 0.04) for im, px in parts)
        out = Image.new('RGBA', (width + 2 * pad, height + 2 * pad), (0, 0, 0, 0))
        if backdrop:      # styled like Interlink's stars/timer panel: a dark slate card with well-rounded corners,
            from PIL import ImageDraw  # darkest on the left and easing to ~40% of that on the right
            shape = Image.new('L', out.size, 0)
            ImageDraw.Draw(shape).rounded_rectangle((0, 0, out.width - 1, out.height - 1), radius=round(H * 0.014), fill=255)
            strength = 255 * backdrop / 100
            fade = Image.linear_gradient('L').rotate(90, expand=True).resize(out.size)
            fade = fade.point(lambda v: round(strength * (1 - 0.6 * v / 255)))  # full on the left, 40% on the right
            card = Image.new('RGBA', out.size, (22, 24, 34, 0))
            from PIL import ImageChops
            card.putalpha(ImageChops.multiply(fade, shape)); out.alpha_composite(card)
        if divider:
            from PIL import ImageDraw
            d = ImageDraw.Draw(out)
            d.line((pad, pad + lw, pad + round(H * 0.215), pad + lw), fill=(20, 26, 40, 90), width=lw)
            d.line((pad, pad, pad + round(H * 0.215), pad), fill=(232, 236, 242, 190), width=lw)
        y = pad + top
        for im, px in parts:
            if isinstance(im, tuple):         # a row: the label's ink on the left edge, the value's on the right one
                label, value = im; base = y + above(im)
                out.alpha_composite(label, (pad - ink(label)[0], base - label.info.get('base', label.height)))
                out.alpha_composite(value, (pad + width - ink(value)[1], base - value.info.get('base', value.height)))
            else:
                x = pad + width - ink(im)[1] if rows else pad + ((width - im.width) // 2 if center else 0)
                out.alpha_composite(im, (x, y))
            y += tall(im) + round(px * 0.04)
        return out

    def styled(c, x, y, text, px, fill='#ffffff', anchor='nw', italic=False, bold=False):
        """Game-like text: a thin dark outline and a soft drop shadow under white (or tinted) text."""
        if GF['font']:
            im = GF['font'].render(text, px, tuple(int(fill[i:i + 2], 16) for i in (1, 3, 5)), italic=italic, bold=bold)
            flat = Image.new('RGB', im.size, KEY_RGB); flat.paste(im, mask=im.getchannel('A'))   # edges blend into the key
            photo = ImageTk.PhotoImage(flat, master=c)
            if not hasattr(c, 'photos'): c.photos = []
            c.photos.append(photo)
            return c.create_image(x, y, image=photo, anchor=anchor)
        font = (FACE, -px); o = max(1, round(px / 26)); d = max(1, round(px / 13))
        c.create_text(x, y + d, text=text, font=font, fill='#141a28', anchor=anchor)          # drop shadow
        for dx, dy in ((-o, 0), (o, 0), (0, -o), (0, o)):
            c.create_text(x + dx, y + dy, text=text, font=font, fill='#28324a', anchor=anchor)  # outline
        return c.create_text(x, y, text=text, font=font, fill=fill, anchor=anchor)

    # --- Stats: plain game-styled text, no box. In Interlink (no minimap) it sits in the top-left corner right of Esc, or
    #     under the stars/timer panel (overlay.rush_position);
    #     out in the world right of the minimap. Drag to move (kept as an offset, a fraction of the game height);
    #     right-click closes the meter.
    dw = tk.Toplevel(root); dw.overrideredirect(True); dw.attributes('-topmost', True)
    dw.configure(bg=KEY); dw.attributes('-transparentcolor', KEY); dw.geometry(OFF)
    dc = tk.Canvas(dw, bg=KEY, highlightthickness=0, width=1, height=1); dc.pack()
    dw.dragging = False
    dps_at = {'base': None, 'h': 1, 'shown': False, 'hide_at': None, 'lines': None, 'key': None}
    drag = {}
    def dps_example(): return bool(meter.args.cfg['overlay'].get('dps_example'))
    # (Overlay › Show an example: it can be dragged while it's up, however the lock is set)
    def dps_locked(): return bool(meter.args.cfg['overlay'].get('dps_locked', True)) and not (dps_example() and not meter.fight)
    def dps_start(e):
        if dps_locked(): return
        x, y = spot(dw)
        drag['x'], drag['y'] = e.x_root - x, e.y_root - y; dw.dragging = True
    def dps_move(e):
        if dw.dragging: move_to(dw, e.x_root - drag['x'], e.y_root - drag['y'], dps_at.get('img'))
    def dps_drop(e=None):
        if not dw.dragging: return
        dw.dragging = False
        if dps_at['base']:
            x, y = spot(dw)
            pos[dps_at['key']] = [(x - dps_at['base'][0]) / dps_at['h'], (y - dps_at['base'][1]) / dps_at['h']]
            save_positions()
    for wdg in (dw, dc):
        wdg.bind('<Button-1>', dps_start); wdg.bind('<B1-Motion>', dps_move)
        wdg.bind('<ButtonRelease-1>', dps_drop); wdg.bind('<Button-3>', quit_all)
    def render_stats(lines, H, divider=False, center=False):
        """lines: [(text, colour, size as a share of the game height, 'i'/'b' style)], under a thin divider when asked
        (out in the world, below the quest list; boss rush has the game's own line under its timer panel)."""
        if GF['font']:
            dps_at['img'] = compose(lines, H, divider, center); return
        dps_at['img'] = None
        dc.delete('all'); dc.photos = []; pad = max(3, round(H * 0.003)); y = pad
        if divider:
            width = round(H * 0.215); lw = max(1, round(H / 1080))
            dc.create_line(pad, y + lw, pad + width, y + lw, fill='#141a28', width=lw)      # shadow
            dc.create_line(pad, y, pad + width, y, fill='#d7dee8', width=lw)
            y += round(H * 0.008)
        rows = any(isinstance(line[0], tuple) for line in lines); right = []    # (items, right edge) to line up
        for text, colour, size, *style in lines:
            px = max(11, round(H * size)); style = style[0] if style else ''
            before = set(dc.find_all())
            if isinstance(text, tuple):       # a row: the label grey on the left, the value moved to the right below
                label = styled(dc, pad, y, text[0], px, '#aab4c3'); before = set(dc.find_all())
                item = styled(dc, dc.bbox(label)[2] + round(H * 0.012), y, text[1], px, colour)
            else: item = styled(dc, pad, y, text, px, colour, italic='i' in style, bold='b' in style)
            if rows: right.append(set(dc.find_all()) - before)
            y = dc.bbox(item)[3] + round(px * 0.12)
        if rows:                              # every value and the big number end on the same right edge
            edge = max(dc.bbox(*items)[2] for items in right)
            for items in right:
                shift = edge - dc.bbox(*items)[2]
                for item in items: dc.move(item, shift, 0)
        box = dc.bbox('all')
        if center:                                    # end screens: each line centred on the widest
            for item in dc.find_all():
                b = dc.bbox(item); dc.move(item, (box[2] - b[2]) // 2, 0)
        if BACKDROP:                                  # without the game font: a solid panel (no partial see-through here)
            dc.tag_lower(dc.create_rectangle(0, 0, box[2] + pad, box[3] + pad, fill=BG, outline=''))
        dc.config(width=box[2] + pad, height=box[3] + pad)
    # The spots (B9), from the game's own HUD layout (its prefabs, as shares of the 16:9 area: x of the width, y of the
    # height), placed clear of where the game shows its own notes and tips:
    #   minimap (0.043-0.144, 0.039-0.218); team panel beside it (0.137-0.264, 0.021-0.069); the AI helper's tips
    #   under that during boss fights (0.136-0.400, 0.141-0.214); boss bar from x 0.302-0.32; one-line system messages
    #   top centre (0.381-0.619, 0.148-0.281); Interlink's stars/timer panel to y 0.326 and its Settle Early button
    #   (0.025-0.194, 0.347-0.391); item and friend toasts on the left from y 0.394-0.455; top right free from x 0.70.
    MINIMAP_RIGHT, BAR_LEFT = 0.195, 0.32     # the party count and menu icons right of the minimap end at ~0.19
    MAP_TOP, MAP_SCALE = 0.069, 0.85          # between the team panel and the AI tips: 0.071 tall, so a little smaller
    def show_dps(screen, bossrush, lines=None, bar=None):
        # the game's HUD is a 16:9 area (centred on ultrawide): spots and text size follow its height, which is the
        # screen's on 16:9 and ultrawide, and the width's 9/16 on 16:10, where the HUD keeps to the top
        ox, oy, W, H = screen; u = min(W, H * 16 / 9); ux = ox + (W - u) / 2; H = u * 9 / 16
        scale = 1.0
        if bossrush and meter.args.cfg['overlay'].get('rush_position') == 'panel':
            base, key = (ux + u * 0.025, oy + H * 0.398), 'dps_offset2_rush'             # under Settle Early
        elif bossrush: base, key = (ux + u * 0.068, oy + H * 0.022), 'dps_offset2_rush_top'  # top left, right of Esc
        elif meter.args.cfg['overlay'].get('world_position') in ('right', 'quests'):
            base, key = (ux + u * 0.705, oy + H * 0.10), 'dps_offset2_right'             # top right, under the buttons
        else:
            # right of the minimap: centred in the gap to the boss bar's left end (where the bar is, else where it
            # usually starts), on a fixed width so it doesn't shift as the numbers change
            scale = MAP_SCALE
            left = ux + u * MINIMAP_RIGHT
            right = bar[0] if bar and left < bar[0] < ux + u * 0.5 else ux + u * BAR_LEFT
            if dps_at.get('bar_left') is None or abs(dps_at['bar_left'] - right) > H * 0.01: dps_at['bar_left'] = right
            width = H * 0.21 * scale
            base, key = (max(left + H * 0.01, (left + dps_at['bar_left']) / 2 - width / 2), oy + H * MAP_TOP), 'dps_offset2_map'
        dps_at['base'], dps_at['h'], dps_at['key'] = base, H, key
        # Overlay › Lock the DPS window: locked, clicks go through it to the game; unlocked, it takes them (to drag it)
        locked = dps_locked()
        if dps_at.get('locked') != locked:
            dps_at['locked'] = locked
            try: layered.click_through(layered.top_hwnd(dw), locked)
            except Exception: pass
        # Overlay › Put the DPS window back: the app bumps dps_reset, and every mode's dragged spot is forgotten
        reset = meter.args.cfg['overlay'].get('dps_reset', 0)
        if reset != pos.get('dps_reset_seen', 0):
            for k in [k for k in pos if k.startswith('dps_offset')]: del pos[k]
            pos['dps_reset_seen'] = reset; save_positions()
        if lines is not None and (lines != dps_at['lines'] or dps_at.get('scale') != scale):
            dps_at['lines'] = lines; dps_at['scale'] = scale; render_stats(lines, H * scale)
        x, y = base; off = pos.get(key)
        # (a spot off the game's area can only be from the old drag, which saved its way off screen: forgotten)
        if off and not offset_on_screen(off, x, y, ox, oy, W, H):
            del pos[key]; save_positions(); off = None
        if off: x += off[0] * H; y += off[1] * H
        if dw.dragging: x, y = spot(dw)
        if dps_at.get('img') is not None: present(dw, dps_at['img'], x, y)
        elif not dw.dragging: place(dw, x, y)
        dps_at['shown'] = True; dps_at['hide_at'] = None
    def set_message(text):
        """A note in the stats spot (fight over, result recorded, Battle Data)."""
        if not dps_at['shown'] or not meter.last_screen: return
        H = dps_at['h']                                # the HUD height show_dps placed it by
        lines = [(t, '#ffffff' if i else '#ffd36a', 0.0165 if i else 0.02) for i, t in enumerate(text.split('\n')) if t]
        dps_at['lines'] = lines; render_stats(lines, H * dps_at.get('scale', 1.0))   # the spot's size (smaller by the minimap)
        if dps_at.get('img') is not None: present(dw, dps_at['img'], *spot(dw))
    def hide_dps(): park(dw); dps_at['shown'] = False
    def end_note_result(d):
        # 'result recorded: Omega Sherro - score 11232, 11 stars, 180s' -> 'Result saved / Omega Sherro · 11,232 · 11 stars · 3:00'
        m = re.match(r'result recorded: (.*) - score (\w+), (\w+) stars, (\w+)s$', d)
        if not m: return 'Result saved'
        boss, score, stars, secs = m.groups()
        parts = [boss] + ([f'{int(score):,}'] if score.isdigit() else []) + ([f'{stars} stars'] if stars.isdigit() else [])
        if secs.isdigit(): parts.append(f'{int(secs) // 60}:{int(secs) % 60:02d}')
        return 'Result saved\n' + ' · '.join(parts)

    def end_note_sim(d):
        # 'Holo-Battle Sim recorded: Earth Training - Trial in 644 s (43 gizmos)' -> 'Holo-Battle Sim saved / Earth Training - Trial · 10:44 · 43 gizmos'
        m = re.match(r'Holo-Battle Sim recorded: (.*) in (\d+) s(?: \((.*)\))?$', d)
        if not m: return 'Holo-Battle Sim saved'
        stage, secs, extra = m.groups()
        return 'Holo-Battle Sim saved\n' + ' · '.join([stage, f'{int(secs) // 60}:{int(secs) % 60:02d}'] + ([extra] if extra else []))

    def end_note_battle_data(d):
        # only the players' facts: the game's team DPS next to the meter's (diagnostics stay in meter.log)
        game = re.search(r'game team DPS ([\d,]+)', d); meter = re.search(r'vs meter ([\d,]+)', d)
        if not game: return 'Battle Data saved'
        return 'Battle Data saved\nTeam DPS ' + game.group(1) + (f' · meter {meter.group(1)}' if meter else '')

    def show_end(text, top):
        """A note on an end screen, centred near the bottom where the screen has room (top = share of the height)."""
        if not show_dps_window or not meter.last_screen: return
        ox, oy, W, H = meter.last_screen
        lines = [(t, '#ffd36a' if i == 0 else '#ffffff', 0.02 if i == 0 else 0.0165) for i, t in enumerate(text.split('\n')) if t]
        dps_at['lines'] = lines; render_stats(lines, H, center=True); dw.update_idletasks()
        img = dps_at.get('img')
        width = img.width if img is not None else dc.winfo_reqwidth()
        x, y = int(ox + W / 2 - width / 2), int(oy + H * top)
        if img is not None: present(dw, img, x, y)
        elif not dw.dragging: place(dw, x, y)
        dps_at['shown'] = True; dps_at['end'] = True

    # --- HP labels: white numbers with a thin black outline and a soft dark shadow, attached to the boss bar; the word HP
    #     in the boss bar's colour where the HP ends, BREAK in its state's colour, both with a thick black outline
    #     (positions are fractions of the bracket span from the left bracket / bar centre line, so they follow the bar
    #     at any resolution). Drag one to nudge it - the offset is kept relative to the bar; double-click resets it.
    HP_SPOTS = {                                      # x, y (span fractions), which side of the text sits there, font px
        'pct':   ((R.TRACK[0] + R.TRACK[1]) / 2, 0.0, 'c', 0.021),   # total HP % left: in the middle of the bar
        'bar':   (1.018, 0.0, 'w', 0.024),            # this bar's HP: right of the xN box
        'total': (1.0, 0.022, 'ne', 0.022),           # total HP left: under the bar, flush with its right bracket
        'break': (1.0, 0.049, 'ne', 0.020),           # break gauge: under the total
    }
    pos.pop('hp', None); pos.pop('dps', None)         # old fixed positions
    hp = {}
    def hp_label(name):
        w = tk.Toplevel(root); w.overrideredirect(True); w.attributes('-topmost', True)
        w.configure(bg=KEY); w.attributes('-transparentcolor', KEY); w.geometry(OFF)
        c = tk.Canvas(w, bg=KEY, highlightthickness=0, width=1, height=1); c.pack()
        L = hp[name] = {'win': w, 'c': c, 'base': None, 'span': 1, 'drag': None}
        def start(e):
            x, y = spot(w); L['drag'] = (e.x_root - x, e.y_root - y)
        def move(e):
            if L['drag']:
                p = placed.get(w)
                move_to(w, e.x_root - L['drag'][0], e.y_root - L['drag'][1], p[1] if p and p[0] == 'img' else None)
        def done(e):
            L['drag'] = None
            if L['base']:
                x, y = spot(w)
                pos['hp_' + name] = [(x - L['base'][0]) / L['span'], (y - L['base'][1]) / L['span']]
                save_positions()
        def reset(e): pos.pop('hp_' + name, None); save_positions()
        for x in (w, c):
            x.bind('<Button-1>', start); x.bind('<B1-Motion>', move); x.bind('<ButtonRelease-1>', done)
            x.bind('<Double-Button-1>', reset)
    for n in HP_SPOTS: hp_label(n)

    def with_shadow(img, px):
        """Text with a soft dark shadow (blurred, a little down and right), so white labels stay readable over bright
        effects and snow. Returns the image and how far the text sits inside it."""
        from PIL import ImageFilter
        blur = max(1, round(px * 0.14)); drop = max(1, round(px * 0.06)); edge = 2 * blur + drop
        out = Image.new('RGBA', (img.width + 2 * edge, img.height + 2 * edge), (0, 0, 0, 0))
        mask = Image.new('L', out.size, 0); mask.paste(img.getchannel('A'), (edge + drop, edge + drop))
        mask = mask.filter(ImageFilter.GaussianBlur(blur)).point(lambda v: min(255, v * 2))      # dense near the text
        shade = Image.new('RGBA', out.size, (6, 8, 16, 0)); shade.putalpha(mask)
        out.alpha_composite(shade); out.alpha_composite(img, (edge, edge))
        return out, edge

    def draw(name, text, bar, colour='#ffffff', hp_rgb=None):
        """Text at its spot next to the bar. bar = (left bracket x, bar centre y, bracket span) on screen.
        (Not `hp=`: that hid the labels' own `hp`, and every label failed to draw, from 0.1.28.)"""
        L = hp[name]; c = L['c']; fx, fy, anchor, fs = HP_SPOTS[name]; bx, by, span = bar
        px = max(11, round(span * fs)); pad = max(2, round(px / 13)) + 3
        if GF['font']:
            # the words HP (in the bar's colour where the HP ends, `hp_rgb`) and BREAK / BROKEN (in their line's colour:
            # yellow when broken) with a thick outline; the numbers with a thin one
            lead = (2, hp_rgb or hp_colour(1.0)) if text.startswith('HP ') else                 (len(text.split(' ', 1)[0]), rgb(colour)) if text.startswith(('BREAK', 'BROKEN')) else None
            img = GF['font'].render(text, px, rgb(colour), bold=name == 'pct', stroke=True, lead=lead)
            W, H = img.size; tx, ty = bx + fx * span, by + fy * span
            x, y = {'w': (tx, ty - H / 2), 'ne': (tx - W, ty), 's': (tx - W / 2, ty - H), 'c': (tx - W / 2, ty - H / 2)}[anchor]
            img, edge = with_shadow(img, px); x -= edge; y -= edge     # the text itself stays on its spot
            L['base'], L['span'] = (x, y), span
            off = pos.get('hp_' + name)
            if off: x += off[0] * span; y += off[1] * span
            if L['drag']: x, y = spot(L['win'])
            present(L['win'], img, x, y); return
        c.delete('all'); c.photos = []
        styled(c, pad, pad, text, px, colour, bold=name == 'pct')
        W, H = c.bbox('all')[2] + pad, c.bbox('all')[3] + pad; c.config(width=W, height=H)
        tx, ty = bx + fx * span, by + fy * span
        x, y = {'w': (tx, ty - H / 2), 'ne': (tx - W, ty), 's': (tx - W / 2, ty - H), 'c': (tx - W / 2, ty - H / 2)}[anchor]
        L['base'], L['span'] = (x, y), span
        off = pos.get('hp_' + name)
        if off: x += off[0] * span; y += off[1] * span
        if not L['drag']: place(L['win'], x, y)
    def clear(name):                                  # parked off screen, not withdrawn: showing it never takes focus
        hp[name]['c'].delete('all'); park(hp[name]['win'])
    def hide_hp():
        for n in hp: clear(n)

    for w in [dw] + [L['win'] for L in hp.values()]: w.update_idletasks()
    hidden = {'on': False, 'done': set()}
    def hide_all():
        """Every overlay window (the DPS window, HP labels, result notes, the alert) out of screen captures: the
        player's choice (General Settings › Overlays show in recordings and screenshots, off)."""
        hidden['on'] = True
        for w in root.winfo_children():
            if not isinstance(w, tk.Toplevel) or w in hidden['done']: continue
            try: wc.hide_from_capture(ctypes.windll.user32.GetParent(w.winfo_id()) or w.winfo_id()); hidden['done'].add(w)
            except Exception: pass
    def set_in_capture(on):
        """Every overlay window in screen captures, or out of them (while a proof is taken)."""
        for w in root.winfo_children():
            if isinstance(w, tk.Toplevel): wc.hide_from_capture(ctypes.windll.user32.GetParent(w.winfo_id()) or w.winfo_id(), not on)
    if hide: hide_all()   # on request; otherwise the meter shows up in screenshots and recordings

    def show_hp(d):
        bar = d.get('bar')
        if not bar or not show_hp_labels: hide_hp(); return
        # the HP left on the bar's boss: the run's max HP less its damage is that boss's (Fight), out of its own max HP
        left = max(0, d['max_hp'] - d['damage']); max_hp = d.get('boss_hp') or d['max_hp'] or 1
        # numbers white with a thin black outline; HP and BREAK with a thick one: HP in the bar's colour where the HP
        # ends, BREAK white, yellow when BROKEN, grey-blue refilling, orange gauge (draw, break_text)
        draw('pct', f'{left / max_hp * 100:.1f}%', bar)
        if d['bossrush'] and d['xn'] and d['seg_size']: draw('bar', f"{fmt(d['seg_left'])} / {fmt(d['seg_size'])}", bar)
        else: clear('bar')
        # the HP bar shows this bar's share in Interlink (xN bars), the whole fight's out in the world
        shown = d['seg_left'] / d['seg_size'] if d['bossrush'] and d['xn'] and d['seg_size'] else left / max_hp
        draw('total', f"HP {fmt(left)} / {fmt(max_hp)}", bar, hp_rgb=hp_colour(shown))
        brk = break_text(d.get('break') or {})
        if brk: draw('break', brk[0], bar, brk[1])
        else: clear('break')

    def clock(s): s = max(0, int(round(s))); return f"{s // 60}:{s % 60:02d}"

    def stat_lines(d):
        """The stats: DPS, then rows for the last 5 s, team damage and the fight's time (dashes until the first hit)."""
        hit = d['damage'] > 0
        return [(f"{fmt(d['avg'])} DPS" if hit else '— DPS', '#ffd36a', 0.0235, 'ib'),  # big italic, like the HUD's
                (('Last 5s', f"{fmt(d['recent'])} DPS" if hit else 'waiting...'), '#ffffff', 0.015),
                (('Team', f"{fmt(d['damage'])} / {fmt(d['max_hp'])} HP"), '#ffffff', 0.015),
                (('Time', clock(d['elapsed'])), '#ffffff', 0.015)] + boss_row(d)

    def final_lines(d):
        """The fight's result, left in the stats spot for a few seconds: the final hit is counted by then. A two-boss
        fight's comes once, at the run's end: there is no fight end between its bosses (Fight.next_boss)."""
        return [(f"{fmt(d['avg'])} DPS", '#ffd36a', 0.0235, 'ib'),
                (('Result', f"Defeated {d['title']}" if d['defeated'] and d.get('title')   # the boss's name on the bar
                  else 'Defeated' if d['defeated'] else 'Fight over'), '#ffffff', 0.015),
                (('Team', f"{fmt(d['damage'])} / {fmt(d['max_hp'])} HP"), '#ffffff', 0.015),
                (('Time', clock(d['elapsed'])), '#ffffff', 0.015)] + boss_row(d)

    def break_text(b):
        """(text, colour) for the break gauge line under the boss bar, or None while it can't be read."""
        if b.get('left') is not None or b.get('breaks'):
            rate = b.get('rate_now') if b.get('rate_now') is not None else b.get('rate')
            count = f" · {b['breaks']} break{'s' if b['breaks'] != 1 else ''}" if b.get('breaks') else ''
            if b.get('phase') == 'broken': text, colour = f"BROKEN{count}", '#ffe066'
            elif b.get('phase') == 'refill': text, colour = f"BREAK gauge refilling{count}", '#c8d0dc'
            else:
                size = b.get('size')
                if size:      # break points from the game data: what's left and break points per second
                    text = (f"BREAK {fmt(b['left'] * size)} / {fmt(size)}"
                            + (f" · {fmt(rate * size / 100)}/s" if rate is not None else '') + count)
                else:
                    text = f"BREAK {b['left'] * 100:.0f}% left" + (f" · {rate:.1f}%/s" if rate is not None else '') + count
                colour = '#ffc58a' if b.get('kind') == 'orange' else '#dfe8f5'
            return text, colour
        return None

    # --- "You're being attacked": while the game isn't in front and your HP bar drops (Meter.hit_watch), shown above
    #     every window the way the game shows it in its menus (notice 50003, "Under
    #     attack", in the tips panel's A3 area): text centred ~26% down the screen at ~2.6% of its height, up for 2 s
    #     and again every 5 s, each time with the red flash around the screen's edges the HUD plays when you're hit
    #     (UI_Ani_Hud_Hitted: opacity 0 > 0.5 > 0.9 > 0 over 0.73 s). Click the text to go back to the game.
    aw = tk.Toplevel(root); aw.overrideredirect(True); aw.attributes('-topmost', True)
    aw.configure(bg=KEY); aw.attributes('-transparentcolor', KEY); aw.geometry(OFF)
    ac = tk.Canvas(aw, bg=KEY, highlightthickness=0, width=1, height=1); ac.pack()
    ew = tk.Toplevel(root); ew.overrideredirect(True); ew.attributes('-topmost', True); ew.geometry(OFF)   # edge flash
    alert = {'screen': None, 'shown': False, 'since': None, 'edge': None, 'alpha': -1}
    ALERT_TEXT, ALERT_COLOUR, ALERT_SIZE, ALERT_Y = "You're being attacked in Aniimo", '#ffffff', 0.026, 0.265
    ALERT_SHOW, ALERT_EVERY = 2.0, 5.0
    HIT_CURVE = ((0.0, 0.0), (0.1, 0.5), (0.233, 0.9), (0.733, 0.0))
    def hit_opacity(t):
        for (t0, v0), (t1, v1) in zip(HIT_CURVE, HIT_CURVE[1:]):
            if t0 <= t <= t1: return v0 + (v1 - v0) * (t - t0) / (t1 - t0)
        return 0.0
    def edge_image(W, H):
        """Red around the screen's edges, fading to nothing towards the middle (drawn small, then enlarged)."""
        w, h = max(16, W // 8), max(9, H // 8); depth = 0.1 * min(w, h)
        ramp = lambda n: [round(220 * max(0.0, 1 - i / depth) ** 2.2) for i in range(n)]
        col, row = Image.new('L', (1, h)), Image.new('L', (w, 1))
        col.putdata([max(a, b) for a, b in zip(ramp(h), ramp(h)[::-1])]); row.putdata([max(a, b) for a, b in zip(ramp(w), ramp(w)[::-1])])
        from PIL import ImageChops
        mask = ImageChops.lighter(col.resize((w, h)), row.resize((w, h))).resize((W, H), Image.BILINEAR)
        img = Image.new('RGBA', (W, H), (235, 30, 30, 0)); img.putalpha(mask); return img
    def back_to_game(e=None):
        if meter.hwnd:
            u32 = ctypes.windll.user32
            if u32.IsIconic(meter.hwnd): u32.ShowWindow(meter.hwnd, 9)            # SW_RESTORE
            u32.SetForegroundWindow(meter.hwnd)
    for wdg in (aw, ac): wdg.bind('<Button-1>', back_to_game)
    def draw_alert_text(ox, oy, W, H, hud):
        if GF['font']:
            px = max(14, round(hud * ALERT_SIZE))
            # the game's tip text: white, with a crisp 40% black copy just below and right of it (its font material's
            # underlay), no outline
            text = GF['font'].render(ALERT_TEXT, px, rgb(ALERT_COLOUR), bold=True)
            dx, dy = max(1, round(px * 0.03)), max(1, round(px * 0.07))
            img = Image.new('RGBA', (text.width + dx, text.height + dy), (0, 0, 0, 0))
            under = Image.new('RGBA', text.size, (0, 0, 0, 0)); under.putalpha(text.getchannel('A').point(lambda v: v * 102 // 255))
            img.alpha_composite(under, (dx, dy)); img.alpha_composite(text, (0, 0))
            layered.show(layered.top_hwnd(aw), img, int(ox + W / 2 - img.width / 2), int(oy + hud * ALERT_Y - img.height / 2))
        else:
            ac.delete('all')
            item = ac.create_text(4, 4, text=ALERT_TEXT, fill=ALERT_COLOUR, anchor='nw',
                                  font=(FACE, -max(14, round(hud * ALERT_SIZE)), 'bold'))
            b = ac.bbox(item); w, h = b[2] + 4, b[3] + 4; ac.config(width=w, height=h)
            aw.geometry(f'+{int(ox + W / 2 - w / 2)}+{int(oy + hud * ALERT_Y - h / 2)}')
    def show_alert(f):
        now = time.time()
        screen = meter.last_screen or (0, 0, root.winfo_screenwidth(), root.winfo_screenheight())
        ox, oy, W, H = screen; hud = min(W, H * 16 / 9) * 9 / 16
        if alert['since'] is None: alert['since'] = now
        t = (now - alert['since']) % ALERT_EVERY
        if t < ALERT_SHOW:
            if not alert['shown'] or alert['screen'] != screen: draw_alert_text(ox, oy, W, H, hud)
            alert['shown'] = True
        elif alert['shown']: aw.geometry(OFF); layered.park(layered.top_hwnd(aw)); alert['shown'] = False
        if alert['screen'] != screen:                     # the flash's picture, once per game screen size
            hw = layered.top_hwnd(ew); layered.show(hw, edge_image(int(W), int(H)), int(ox), int(oy))
            layered.click_through(hw); alert['alpha'] = 255
        alert['screen'] = screen
        a = round(255 * hit_opacity(t))
        if a != alert['alpha']: layered.set_alpha(layered.top_hwnd(ew), a); alert['alpha'] = a
    def hide_alert():
        if alert['since'] is None: return
        aw.geometry(OFF); layered.park(layered.top_hwnd(aw)); alert['shown'] = False; alert['since'] = None
        if alert['screen'] is not None: layered.set_alpha(layered.top_hwnd(ew), 0); alert['alpha'] = 0

    # every plug-in's notices (the core's; notice_window.py)
    tw, toast, toast_tick = notice_window(tk, root, meter, hide, AWAY, present, park, pos, save_positions, OFF, rgb)

    ours = set()
    def game_in_front():
        """The game (or one of the overlay's own windows, e.g. while dragging it) is the active window."""
        if not meter.hwnd or dw.dragging or any(L['drag'] for L in hp.values()): return True
        if not ours: ours.update(layered.top_hwnd(w) for w in [dw, aw, ew, tw] + [L['win'] for L in hp.values()])
        if toast['drag']: return True
        fg = ctypes.windll.user32.GetForegroundWindow()
        return fg == meter.hwnd or fg in ours

    def show_fight(d):
        dps_at['bossrush'] = d['bossrush']
        if show_dps_window: show_dps(d['screen'], d['bossrush'], stat_lines(d), d.get('bar'))
        if time.time() - meter.bar_seen < BAR_HIDDEN: show_hp(d); dps_at['hp_up'] = True

    # Overlay › Show an example: the DPS window with made-up numbers, over the game, so it can be placed without a fight. It's
    # at the spot for where you are (Interlink's, or the world's), and a fight's real numbers take over while they're up.
    EXAMPLE = {'avg': 12345, 'recent': 15678, 'damage': 987654, 'max_hp': 2500000, 'elapsed': 80, 'title': None}
    def show_example():
        w, h = meter.frame_size
        on = show_dps_window and dps_example() and not meter.fight and not dps_at['hide_at'] and w > 0 and h > 0
        if on:
            now = time.time()
            if now - dps_at.get('example_at', 0) >= OVERLAY_REDRAW:      # (its place only changes with the settings)
                dps_at['example_at'] = now; dps_at['example'] = True
                show_dps((meter.origin[0], meter.origin[1], w, h), bool(meter.bossrush_space), stat_lines(EXAMPLE))
        elif dps_at.get('example'):
            dps_at['example'] = False
            if not meter.fight and not dps_at['hide_at']: hide_dps()
    poll_error = {'at': 0.0}
    def poll():
        if not th.is_alive(): root.destroy(); return          # the meter stopped (asked to, or closed): exit
        # an error in one tick is logged (once a minute) and the next tick runs anyway: it used to end the loop with
        # everything frozen as it was ("You're being attacked" stuck over other windows for minutes)
        try: poll_tick()
        except Exception:
            # the DPS meter's (logged [dps], counted): three in a minute stop it, and its windows go (the notices stay).
            # Stopped, what still fails is logged once a minute
            if not HEALTH.failed('dps'): HEALTH.error('dps', sys.exc_info()[1], 'overlay')
            elif time.time() - poll_error['at'] > 60:
                poll_error['at'] = time.time(); log.exception('overlay: error with the DPS meter stopped (it keeps going)')
        root.after(15, poll)    # tabbing in/out and new readings show within a frame or two
    drawn = {'at': 0.0, 'pending': None}     # the overlay's last redraw, and the newest reading waiting for its turn
    def poll_tick():
        # the app's picture is of the screen (Desktop Duplication): keep our windows out of it, or the meter would
        # read its own HP labels on the boss bar
        # the HP labels sit on the boss bar, which the meter must read: those stay out of captures. The DPS window stays
        # in screenshots and recordings (unless the player chose otherwise), blacked out of the meter's own pictures
        if time.time() - own.get('published', 0) > 1: publish_own(time.time())
        # Every overlay window shows in screenshots and recordings (unless the player chose otherwise), on any Windows.
        # Where the meter's picture is of the screen (Desktop Duplication) it covers them in its own copy (mask_own),
        # and they step out of captures only while a proof is taken, so a proof is the game alone.
        if hide: hide_all(); meter.own_in_capture = False
        else:
            want_in = not (meter.use_screen and meter.capture_clean())
            if want_in != meter.own_in_capture:
                # in: the meter starts covering them first, then they show in captures; out: they leave captures, then
                # the covering stops (proofs wait SETTLE_S before their first look, many frames after this)
                if want_in: meter.own_in_capture = True
                try:
                    set_in_capture(want_in); meter.own_in_capture = want_in
                except Exception: meter.own_in_capture = True if want_in else None   # unknown: keep covering, try again
        front = game_in_front()
        if AWAY['on'] != (not front):
            AWAY['on'] = not front
            for w, (kind, *where) in list(placed.items()):
                if AWAY['on']: w.geometry(OFF)                         # tabbed out: out of the way
                elif kind == 'img': layered.show(layered.top_hwnd(w), *where)
                else: w.geometry(f'+{int(where[0])}+{int(where[1])}')
        if getattr(meter, 'quiet', False) or getattr(meter, 'capture_off', False) or HEALTH.failed('dps'):
            # the DPS meter is off (or stopped for errors), whether or not the leaderboards still read the game: none of
            # its windows, its messages dropped; the notices (every plug-in's) go on
            if not dps_at.get('quiet'):
                dps_at['quiet'] = True
                hide_alert(); hide_hp(); hide_dps(); dps_at['hide_at'] = None; dps_at['end'] = False
                drawn['pending'] = None
            try:
                while True:
                    k, d = q.get_nowait()
                    if k == 'toast': toast['queue'].append(d)
            except queue.Empty: pass
            toast_tick(); return
        dps_at['quiet'] = False
        f = meter.fight
        if AWAY['on'] and time.time() - getattr(meter, 'hit_at', 0) < 5:
            show_alert(f)                                               # tabbed out and your HP just dropped
        else: hide_alert()
        latest = None                                          # of several fight frames waiting, only the newest is drawn
        try:
            while True:
                k, d = q.get_nowait()
                if k == 'fight': latest = d; continue
                if k == 'toast': toast['queue'].append(d); continue
                if latest is not None: show_fight(latest); latest = None; drawn['pending'] = None; drawn['at'] = time.time()
                if k == 'fight_end':                                    # the result stays up for a while
                    drawn['pending'] = None                              # a reading still waiting is from before the end
                    hide_hp()
                    keep = 10 if d['bossrush'] else 0 if d.get('holosim') else meter.args.cfg['overlay'].get('result_seconds', 10)
                    if show_dps_window and dps_at['shown'] and keep > 0:
                        show_dps(d['screen'], d['bossrush'], final_lines(d), d.get('bar')); dps_at['hide_at'] = time.time() + keep
                    elif dps_at['shown']: hide_dps()
                elif k == 'status' and d.startswith('fight over'):
                    hide_hp()
                    if dps_at['shown'] and not dps_at['hide_at']: hide_dps()   # no final numbers came (meter closed)
                elif k == 'status' and d.startswith('result recorded'):
                    # result screen: between the player row and the "automatically exit" line
                    show_end(end_note_result(d), 0.665)
                    dps_at['hide_at'] = time.time() + 30
                elif k == 'status' and d.startswith('Holo-Battle Sim recorded'):
                    # Victory screen: between the panel and "press anywhere to close"
                    show_end(end_note_sim(d), 0.875)
                    dps_at['hide_at'] = time.time() + 15
                elif k == 'status' and d.startswith('Battle Data'):
                    # Battle Data: under the player cards
                    show_end(end_note_battle_data(d), 0.765)
                    dps_at['hide_at'] = time.time() + 15
                elif k == 'end_hide':
                    if dps_at.get('end'): hide_dps(); dps_at['end'] = False
        except queue.Empty: pass
        # the numbers redrawn at most 4 times a second (B8: every new reading, up to 60 a second, redrew the window);
        # the newest waits for its turn, and a fight's end shows at once (above)
        if latest is not None: drawn['pending'] = latest
        toast_tick()
        if drawn['pending'] is not None and time.time() - drawn['at'] >= OVERLAY_REDRAW:
            show_fight(drawn['pending']); drawn['pending'] = None; drawn['at'] = time.time()
        if meter.fight and time.time() - meter.bar_seen >= BAR_HIDDEN and dps_at.get('hp_up'): hide_hp(); dps_at['hp_up'] = False
        # Holo-Battle Sim: the numbers too, while its bar is gone (the floor's menus, gizmo picks, the next floor)
        if (meter.fight and meter.holosim_space and dps_at['shown'] and not dps_at['hide_at']
                and time.time() - meter.bar_seen >= 2): hide_dps()
        if dps_at['hide_at'] and time.time() > dps_at['hide_at'] and not meter.fight: hide_dps(); dps_at['hide_at'] = None
        show_example()
    th = threading.Thread(target=meter.run, daemon=True); th.start()
    def backstop():
        # the meter stopped (aLogs closed or turned it off) but the window loop didn't end: never stay behind with
        # an overlay frozen on screen (the alert stayed up after aLogs was closed)
        th.join(); time.sleep(3); log.info('meter stopped but the overlay hung: exiting'); os._exit(0)
    threading.Thread(target=backstop, daemon=True).start()
    poll(); root.mainloop()
    meter.stop = True; th.join(3)
