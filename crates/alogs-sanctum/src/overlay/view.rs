//! What the overlay draws: the full map lined up over the game's map (or a centred panel with a legend), and the
//! on-screen map while it's closed (corner, round, radar, compass, see-through, or the big map). Port of
//! `OverlayView` in OverlayWindow.cs (Dev switches off: no retained map, no chest sets, no boss rooms).

use super::canvas::{Canvas, Fill, Pen, Rgba, ellipse, line, polygon, rounded_rect};
use super::text;
use crate::cv::{self, Interp};
use crate::cv2::{self, Kernel};
use crate::detectors::{self, Anchor};
use crate::engine::OverlayFrame;
use crate::lang;
use crate::layout_db::{Layout, LayoutDb, Room};
use crate::settings::Settings;
use crate::vision::{self, P2};
use std::collections::HashMap;
use std::sync::Arc;
use tiny_skia::Pixmap;


pub fn color(name: &str) -> Rgba {
    match name {
        "orange" => Rgba::rgb(0xFF, 0x8A, 0x1E),
        "blue" => Rgba::rgb(0x3D, 0x8B, 0xFF),
        "nest" => Rgba::rgb(0xFF, 0x4F, 0xD8),
        "key" | "legendary" => Rgba::rgb(0xFF, 0xD3, 0x3A),
        "luxurious" => Rgba::rgb(0xFF, 0x9F, 0x43),
        "exquisite" => Rgba::rgb(0x5C, 0xC8, 0xFF),
        "simple" => Rgba::rgb(0xC8, 0xC8, 0xC8),
        "pot" => Rgba::rgb(0xC9, 0x8B, 0x5A),
        "pile" => Rgba::rgb(0x9B, 0xE5, 0x6A),
        "mound" => Rgba::rgb(0x9B, 0x86, 0x6A),
        "player" => Rgba::rgb(0x40, 0xFF, 0xD0),
        _ => Rgba::WHITE,
    }
}

const PANEL_FILL: Rgba = Rgba(14, 16, 26, 215);
const RADAR_FILL: Rgba = Rgba(6, 18, 28, 200);
const RADAR_RING: Rgba = Rgba(120, 210, 255, 70);
const DISC: Rgba = Rgba(24, 27, 40, 225);
const DISC_RING: Rgba = Rgba(255, 255, 255, 150);
const BOAT: Rgba = Rgba::rgb(0xB8, 0xF5, 0x6E);
const PALE: Rgba = Rgba::rgb(0xE9, 0xEA, 0xF2);

/// The on-screen map styles, in the order the settings list them: (value, name, description).
pub const MINI_MAP_STYLES: [(&str, &str, &str); 6] = [
    ("Corner", "Corner map", "A framed north-up map of the area around you, like Path of Exile. Drag it anywhere."),
    ("Disc", "Round minimap", "A round map that turns with you, so up is always the way you face."),
    ("Radar", "Radar", "Just the doors, key rooms and nests around you on a faint outline, with arrows and distances to the ones out of range."),
    ("Compass", "Compass strip", "A strip along the top of the screen showing which way, and how far, the doors, key rooms and nests are."),
    ("Overlay", "See-through (Diablo)", "Faint wall outlines over the middle of the screen, centered on your character."),
    ("Off", "Off", "No on-screen map while the full map is closed."),
];

/// The top of the game the meter reads: the big map and the see-through outlines stay below it.
pub const METER_BAND: f64 = 0.13;

/// (x, y, w, h).
pub type R = (f64, f64, f64, f64);

fn meets(a: R, b: R) -> bool {
    a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
}

pub fn difficulty_color(d: Option<&str>) -> Rgba {
    match d {
        Some("Normal") => Rgba::rgb(0x9B, 0xE8, 0x8F),
        Some("Hard") => Rgba::rgb(0xFF, 0xC8, 0x5C),
        Some("Nightmare") => Rgba::rgb(0xFF, 0x7A, 0x6B),
        Some("Chaos") => Rgba::rgb(0xD3, 0x8B, 0xFF),
        _ => PALE,
    }
}

fn key_color(r: &Room) -> Rgba {
    match r.door.as_ref().map(|d| d.color.as_str()) {
        Some("purple") => Rgba::rgb(0xC0, 0x7C, 0xFF),
        Some("blue") => Rgba::rgb(0x5C, 0xB8, 0xFF),
        _ => color("key"),
    }
}

fn dist(img: &[f64], me: P2) -> f64 {
    ((img[0] - me.0).powi(2) + (img[1] - me.1).powi(2)).sqrt()
}

/// Big map: a large panel in the middle, below the meter's band and above the game's bar at the bottom.
pub fn big_panel(s: &Settings, w: f64, h: f64, ui: f64) -> R {
    let (top, bottom) = (METER_BAND * h + 24.0 * ui, 0.9 * h);
    let size = (s.big_map_size * h).min(w - 2.0 * 340.0 * ui).min(bottom - top).max(1.0);
    let y = ((h - size) / 2.0).clamp(top, top.max(bottom - size));
    ((w - size) / 2.0, y, size, size)
}

/// Where the see-through outlines can show: the circle they fade out in, from the meter's band down.
pub fn see_through_area(w: f64, h: f64) -> R {
    let (r, cx, cy, top) = (h * 0.55, w / 2.0, h * 0.56, METER_BAND * h);
    let (x0, y0) = ((cx - r).max(0.0), top.max(0.0));
    let (x1, y1) = ((cx + r).min(w), (cy + r).min(h));
    if x1 <= x0 || y1 <= y0 { (0.0, top, w, h - top) } else { (x0, y0, x1 - x0, y1 - y0) }
}

/// Keeps a panel on screen, snaps it to a near edge, and moves it out of the game's minimap / quest area.
pub fn fit(r: R, cw: f64, ch: f64, ui: f64) -> R {
    let (m, snap) = (12.0 * ui, 28.0 * ui);
    let mut x = r.0.clamp(m, m.max(cw - r.2 - m));
    let mut y = r.1.clamp(m, m.max(ch - r.3 - m));
    if x - m < snap {
        x = m;
    }
    if cw - m - (x + r.2) < snap {
        x = cw - m - r.2;
    }
    if y - m < snap {
        y = m;
    }
    if ch - m - (y + r.3) < snap {
        y = ch - m - r.3;
    }
    let hud = (vision::hud(cw as usize, ch as usize).0 as f64, 0.0, 300.0 * ui, 260.0 * ui);
    if meets((x, y, r.2, r.3), hud) {
        let (right, down) = (hud.0 + hud.2 + m - x, hud.1 + hud.3 + m - y);
        if right <= down {
            x += right;
        } else {
            y += down;
        }
    }
    (x, y, r.2, r.3)
}

/// A target the radar and compass point at: icon, where (image px), colour, size.
type Target = (&'static str, Vec<f64>, Rgba, f64);

pub struct OverlayView {
    pub db: Arc<LayoutDb>,
    images: HashMap<(i64, usize), Arc<Pixmap>>,
    outlines: HashMap<(i64, usize), Arc<Pixmap>>,
    icons: HashMap<String, Option<Arc<Pixmap>>>,
    /// Drawn inside the settings window: no "not lined up" hint.
    pub preview: bool,
    pub preview_ui: f64,
    /// Where the window sits inside the game client when it only covers the map panel.
    pub offset: (f64, f64),
    pub placing: bool,
    /// Game client (or screen) covered while placing.
    pub placement_client: (i32, i32, i32, i32),
    pub panel_rect: R,
    pub done_button: R,
    pub reset_button: R,
    pub cancel_button: R,
    /// Twined with an egg (the player is drawn as the egg marker).
    pub egg: bool,
    /// Set while drawing inside a rotated map: icons and labels are turned back by this much.
    spin: f64,
}

/// A BGRA (or BGR) picture as a premultiplied pixmap.
pub fn to_pixmap(m: &cv::Mat) -> Option<Pixmap> {
    let mut p = Pixmap::new(m.w as u32, m.h as u32)?;
    let out = p.data_mut();
    for (i, px) in m.data.chunks_exact(m.c).enumerate() {
        let (b, g, r, a) = match m.c {
            4 => (px[0], px[1], px[2], px[3]),
            3 => (px[0], px[1], px[2], 255),
            _ => (px[0], px[0], px[0], 255),
        };
        let pm = |c: u8| ((u16::from(c) * u16::from(a) + 127) / 255) as u8;
        out[i * 4..i * 4 + 4].copy_from_slice(&[pm(r), pm(g), pm(b), a]);
    }
    Some(p)
}

impl OverlayView {
    pub fn new(db: Arc<LayoutDb>) -> OverlayView {
        OverlayView {
            db,
            images: HashMap::new(),
            outlines: HashMap::new(),
            icons: HashMap::new(),
            preview: false,
            preview_ui: 0.9,
            offset: (0.0, 0.0),
            placing: false,
            placement_client: (0, 0, 0, 0),
            panel_rect: (0.0, 0.0, 0.0, 0.0),
            done_button: (0.0, 0.0, 0.0, 0.0),
            reset_button: (0.0, 0.0, 0.0, 0.0),
            cancel_button: (0.0, 0.0, 0.0, 0.0),
            egg: false,
            spin: 0.0,
        }
    }

    /// Drops the decoded pictures of other layouts (the island's stay: the player goes back and forth).
    fn forget(&mut self, keep: i64, outlines: bool) {
        let island = self.db.island().map_or(i64::MIN, |l| l.number);
        let c = if outlines { &mut self.outlines } else { &mut self.images };
        c.retain(|k, _| k.0 == keep || k.0 == island);
    }

    /// Lets go of every decoded picture (the map hidden for a while).
    pub fn free(&mut self) {
        self.images.clear();
        self.outlines.clear();
    }

    /// A floor's art (the island's at half size: plenty for the on-screen map, cheap to redraw).
    fn art(&mut self, l: &Layout, floor: usize) -> Option<Arc<Pixmap>> {
        let floor = floor.min(LayoutDb::floor_count(l) - 1);
        if let Some(p) = self.images.get(&(l.number, floor)) {
            return Some(p.clone());
        }
        let mut m = vision::imread(&self.db.floor_path(l, floor))?;
        if l.is_island() && m.w > 1 {
            let w = (l.image_size.first().copied().unwrap_or(m.w as i64) / 2).max(1) as usize;
            let h = ((m.h as f64) * w as f64 / m.w as f64).round().max(1.0) as usize;
            m = cv::resize(&m, w, h, 0.0, 0.0, Interp::Area);
        }
        let p = Arc::new(to_pixmap(&m)?);
        self.forget(l.number, false);
        self.images.insert((l.number, floor), p.clone());
        Some(p)
    }

    /// Wall outlines of a layout (Diablo-style): light edges with a faint floor tint, transparent elsewhere.
    fn outline(&mut self, l: &Layout, level: usize) -> Option<Arc<Pixmap>> {
        let level = level.min(LayoutDb::floor_count(l) - 1);
        if let Some(p) = self.outlines.get(&(l.number, level)) {
            return Some(p.clone());
        }
        let rgba = vision::imread(&self.db.floor_path(l, level))?;
        let gray = cv::bgr2gray(&rgba);
        let alpha = if rgba.c == 4 { rgba.channel(3) } else { cv::Mat { data: vec![255; gray.data.len()], ..gray.clone() } };
        let floor = cv::bitwise_and(&cv::threshold(&gray, 120.0, 255.0, cv::Thresh::Binary), &alpha);
        let k = Kernel::ellipse(5, 5);
        let floor = cv2::open(&floor, &k);
        let edge = cv2::gradient(&floor, &k);
        let mut p = Pixmap::new(gray.w as u32, gray.h as u32)?;
        let d = p.data_mut();
        for i in 0..gray.data.len() {
            // premultiplied RGBA: faint floor tint, soft blue-white walls
            let px: [u8; 4] = if edge.data[i] != 0 { [190, 225, 235, 235] } else if floor.data[i] != 0 { [22, 26, 30, 34] } else { [0, 0, 0, 0] };
            d[i * 4..i * 4 + 4].copy_from_slice(&px);
        }
        let p = Arc::new(p);
        self.forget(l.number, true);
        self.outlines.insert((l.number, level), p.clone());
        Some(p)
    }

    fn icon_image(&mut self, name: &str) -> Option<Arc<Pixmap>> {
        if let Some(i) = self.icons.get(name) {
            return i.clone();
        }
        let p = vision::imread(&self.db.root.join("icons").join(format!("{name}.png"))).and_then(|m| to_pixmap(&m)).map(Arc::new);
        self.icons.insert(name.to_string(), p.clone());
        p
    }

    /// Where a floor image goes: the ground floor spans the layout's full image size; upper floors their own size.
    fn image_rect(l: &Layout, floor: usize, bmp: &Pixmap, tx: f64, ty: f64, scale: f64) -> R {
        if floor == 0 || LayoutDb::floor_count(l) == 1 {
            (tx, ty, l.image_size[0] as f64 * scale, l.image_size[1] as f64 * scale)
        } else {
            (tx, ty, f64::from(bmp.width()) * scale, f64::from(bmp.height()) * scale)
        }
    }

    /// The style actually drawn: no see-through on the busy island art, and placing shows a panel to drag.
    pub fn mini_style(&self, s: &Settings, f: &OverlayFrame, l: &Layout) -> String {
        if f.big {
            return "Big".into();
        }
        let mut st = s.mini_map_mode.clone();
        if !MINI_MAP_STYLES.iter().any(|x| x.0 == st) {
            st = "Corner".into();
        }
        if (st == "Overlay" || st == "Off") && (l.is_island() || self.placing) {
            st = "Corner".into();
        }
        st
    }

    fn covers_game_map(&self, s: &Settings, style: &str) -> bool {
        s.mini_map_corner == "OverGame" && (style == "Corner" || style == "Disc") && !self.placing
    }

    fn mini_opacity(&self, s: &Settings) -> f64 {
        if s.mini_map_corner == "OverGame" && (s.mini_map_mode == "Corner" || s.mini_map_mode == "Disc") && !self.placing { 1.0 } else { s.mini_map_opacity }
    }

    fn panel_size(s: &Settings, style: &str, ui: f64) -> (f64, f64) {
        let size = s.mini_map_size * ui;
        if style == "Compass" { (size * 2.4, 62.0 * ui) } else { (size, size) }
    }

    /// Where the panel goes: the user's spot if placed, else top right clear of the party portraits (or top centre).
    pub fn mini_panel(&self, s: &Settings, style: &str, cw: f64, ch: f64, ui: f64) -> R {
        if self.covers_game_map(s, style) {
            let m = detectors::to_client(s.minimap_region, Anchor::TopLeft, cw as usize, ch as usize);
            let side = m.2.max(m.3) as f64;
            return (m.0 as f64 + m.2 as f64 / 2.0 - side / 2.0, m.1 as f64 + m.3 as f64 / 2.0 - side / 2.0, side, side);
        }
        let sz = Self::panel_size(s, style, ui);
        let c = if let (Some(fx), Some(fy)) = (s.mini_map_x, s.mini_map_y) {
            (fx * cw, fy * ch)
        } else if style == "Compass" {
            (cw / 2.0, 96.0 * ui)
        } else {
            let (hx, _, hw, _) = vision::hud(cw as usize, ch as usize);
            let x = if s.mini_map_corner == "TopCenter" { (cw - sz.0) / 2.0 } else { (hx + hw) as f64 - sz.0 - 240.0 * ui };
            (x + sz.0 / 2.0, 105.0 * ui + sz.1 / 2.0)
        };
        fit((c.0 - sz.0 / 2.0, c.1 - sz.1 / 2.0, sz.0, sz.1), cw, ch, ui)
    }

    /// A sample frame to drag around when nothing is being tracked.
    fn sample_frame(&self) -> Option<OverlayFrame> {
        let c = self.placement_client;
        let l = self.db.by_number(53).or_else(|| self.db.layouts.iter().find(|x| !x.is_island()))?;
        let at = l.key_rooms().next().and_then(|r| r.door.as_ref().map(|d| d.img.clone())).or_else(|| l.orange().map(|o| o.img.clone()))?;
        Some(OverlayFrame {
            client: (c.0, c.1, c.2.max(1), c.3.max(1)),
            visible: true,
            layout: Some(l.number),
            player: Some((at[0] + 40.0, at[1] + 30.0)),
            player_fresh: true,
            mini: true,
            heading: Some(35.0),
            ..Default::default()
        })
    }

    /// Draws a frame onto a canvas the size of the window. `shown`: where the on-screen map has the player now
    /// (its glide), and the turned arrow and cone.
    pub fn render(&mut self, cv: &mut Canvas, s: &Settings, frame: &OverlayFrame, shown: Option<P2>, shown_heading: Option<f64>, shown_camera: Option<f64>) {
        let mut f = frame.clone();
        if self.placing {
            let live = f.visible && f.mini && !f.big && f.layout.is_some() && f.player.is_some();
            if !live {
                match self.sample_frame() {
                    Some(sf) => f = sf,
                    None => return,
                }
            } else if self.placement_client.2 > 0 {
                f.client = self.placement_client;
            }
        }
        if !f.visible {
            return;
        }
        let pushed = !(self.placing || self.preview);
        if pushed {
            cv.push_translate(-self.offset.0, -self.offset.1);
        }
        let ui = if self.preview && !f.mini { self.preview_ui } else { vision::ui_scale(f.client.2.max(1) as usize, f.client.3.max(1) as usize) };
        let ms = s.marker_scale * ui;
        if f.mini
            && let (Some(l), Some(me)) = (f.layout.and_then(|n| self.db.by_number(n).cloned()), f.player)
        {
            if !self.preview {
                f.heading = shown_heading.or(f.heading);
                f.camera = shown_camera.or(f.camera);
            }
            let at = if self.preview { me } else { shown.unwrap_or(me) };
            self.render_mini(cv, s, &f, &l, at, ui);
        } else {
            self.render_full(cv, s, &f, ui, ms);
        }
        if pushed {
            cv.pop_transform();
        }
    }

    fn render_full(&mut self, cv: &mut Canvas, s: &Settings, f: &OverlayFrame, ui: f64, ms: f64) {
        let aligned = f.pose.is_some();
        let layout = f.pose.as_ref().map(|p| p.layout).or(f.layout).and_then(|n| self.db.by_number(n).cloned());
        let Some(l) = layout else {
            self.banner(cv, f, ui, &f.status);
            return;
        };
        let (scale, tx, ty);
        if let Some(p) = &f.pose {
            (scale, tx, ty) = (p.scale, p.tx, p.ty);
        } else {
            // fit the occupied part of the map into 80% of the screen
            let pts: Vec<&Vec<f64>> = if l.rooms.is_empty() { l.markers.iter().map(|m| &m.img).collect() } else { l.rooms.iter().map(|r| &r.img).collect() };
            if pts.is_empty() {
                return;
            }
            let min_x = pts.iter().map(|p| p[0]).fold(f64::MAX, f64::min) - 120.0;
            let max_x = pts.iter().map(|p| p[0]).fold(f64::MIN, f64::max) + 120.0;
            let min_y = pts.iter().map(|p| p[1]).fold(f64::MAX, f64::min) - 120.0;
            let max_y = pts.iter().map(|p| p[1]).fold(f64::MIN, f64::max) + 120.0;
            let fill = if self.preview { 0.92 } else { 0.8 };
            scale = (f64::from(f.client.2) * fill / (max_x - min_x)).min(f64::from(f.client.3) * fill / (max_y - min_y));
            tx = f64::from(f.client.2) / 2.0 - (min_x + max_x) / 2.0 * scale;
            ty = f64::from(f.client.3) / 2.0 - (min_y + max_y) / 2.0 * scale;
            if !self.preview {
                let r = rounded_rect(min_x * scale + tx - 16.0, min_y * scale + ty - 16.0, (max_x - min_x) * scale + 32.0, (max_y - min_y) * scale + 32.0, 14.0);
                cv.fill(r, &Fill::Solid(Rgba(12, 14, 22, 200)));
            }
        }
        let p = |img: &[f64]| (img[0] * scale + tx, img[1] * scale + ty);
        if (s.show_map_image && !l.is_island()) || !aligned {
            if let Some(bmp) = self.art(&l, f.floor) {
                let r = Self::image_rect(&l, f.floor, &bmp, tx, ty, scale);
                cv.image(&bmp, r.0, r.1, r.2, r.3, if aligned { s.map_opacity } else { 0.95 });
            }
        }
        self.draw_markers(cv, s, &l, &p, ms, s.show_labels);
        if s.show_player
            && let Some(pl) = f.player
        {
            let at = (pl.0 * scale + tx, pl.1 * scale + ty);
            self.player(cv, at, f.heading, 40.0 * ms, f.player_fresh);
            if s.show_labels {
                self.label(cv, &lang::t("You"), (at.0, at.1 - 22.0 * ms), color("player"), 12.0 * ms, true);
            }
        }
        self.legend(cv, f, &l, ui, aligned);
    }

    fn render_mini(&mut self, cv: &mut Canvas, s: &Settings, f: &OverlayFrame, l: &Layout, me: P2, ui: f64) {
        let style = self.mini_style(s, f, l);
        let center = style == "Overlay";
        // same metres per screen pixel on the Sanctum (6.1 px/m art) and the island (4 px/m)
        let zoom = if f.big { s.big_map_zoom } else if center { s.overlay_zoom } else { s.mini_map_zoom };
        let scale = zoom * ui * (6.1 / l.px_per_metre().max(1.0));
        let ms = s.marker_scale * ui * if f.big { 0.85 } else if center { 0.75 } else { 0.6 };
        let (cw, ch) = (f64::from(f.client.2), f64::from(f.client.3));
        let panel = if center {
            (0.0, 0.0, cw, ch)
        } else if f.big {
            big_panel(s, cw, ch, ui)
        } else {
            self.mini_panel(s, &style, cw, ch, ui)
        };
        self.panel_rect = panel;
        match style.as_str() {
            // the character stands a little below the screen centre in the game's camera
            "Overlay" => self.render_see_through(cv, s, f, l, me, (cw / 2.0, ch * 0.56), scale, ms, ui),
            "Disc" => self.render_disc(cv, s, f, l, me, panel, scale, ms, ui),
            "Radar" => self.render_radar(cv, s, f, l, me, panel, scale, ms, ui),
            "Compass" => self.render_compass(cv, s, f, l, me, panel, ui),
            _ => self.render_panel(cv, s, f, l, me, panel, scale, ms, ui),
        }
        if self.placing {
            self.placement_chrome(cv, panel, &style, ui);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_panel(&mut self, cv: &mut Canvas, s: &Settings, f: &OverlayFrame, l: &Layout, me: P2, panel: R, scale: f64, ms: f64, ui: f64) {
        let c = (panel.0 + panel.2 / 2.0, panel.1 + panel.3 / 2.0);
        let (tx, ty) = (c.0 - me.0 * scale, c.1 - me.1 * scale);
        let p = |img: &[f64]| (img[0] * scale + tx, img[1] * scale + ty);
        let clip = rounded_rect(panel.0, panel.1, panel.2, panel.3, 18.0 * ui);
        cv.push_opacity(self.mini_opacity(s));
        cv.fill(clip.clone(), &Fill::Solid(PANEL_FILL));
        if let Some(cl) = &clip {
            cv.push_clip(cl);
        }
        if let Some(bmp) = self.art(l, f.floor) {
            let r = Self::image_rect(l, f.floor, &bmp, tx, ty, scale);
            cv.image(&bmp, r.0, r.1, r.2, r.3, 0.9);
        }
        self.draw_markers(cv, s, l, &p, ms, f.big && s.show_labels);
        // you: the game's arrow, turned the way you face, under the camera's view cone
        self.view_cone(cv, s, c, f.camera, if f.big { 130.0 } else { 70.0 } * ui);
        self.player(cv, c, f.heading, if f.big { 44.0 } else { 34.0 } * ui, true);
        if clip.is_some() {
            cv.pop_layer();
        }
        cv.stroke(rounded_rect(panel.0, panel.1, panel.2, panel.3, 18.0 * ui), Pen::new(Rgba(255, 255, 255, 160), 1.5 * ui));
        self.label(cv, "N", (panel.0 + panel.2 / 2.0, panel.1 + 4.0 * ui), Rgba::WHITE, 13.0 * ui, false);
        self.caption(cv, s, f, l, panel, ui, 0.0);
        cv.pop_layer();
    }

    /// Diablo-style: faint outlines fading out towards the screen edges.
    #[allow(clippy::too_many_arguments)]
    fn render_see_through(&mut self, cv: &mut Canvas, s: &Settings, f: &OverlayFrame, l: &Layout, me: P2, c: P2, scale: f64, ms: f64, ui: f64) {
        let (tx, ty) = (c.0 - me.0 * scale, c.1 - me.1 * scale);
        let p = |img: &[f64]| (img[0] * scale + tx, img[1] * scale + ty);
        let outline = self.outline(l, f.floor);
        let ch = f64::from(f.client.3);
        let r = (ch * 0.55) as f32;
        let black = Rgba(0, 0, 0, 255);
        let clear = Rgba(0, 0, 0, 0);
        let area = see_through_area(f64::from(f.client.2), ch);
        // out towards the top, gone by the meter's band
        cv.push_opacity_mask(&Fill::Linear((0.0, area.1 as f32), (0.0, (area.1 + 0.08 * ch) as f32), vec![(0.0, clear), (1.0, black)]));
        cv.push_opacity_mask(&Fill::Radial(c.0 as f32, c.1 as f32, r, vec![(0.0, black), (0.45, black), (1.0, clear)]));
        cv.push_opacity(s.overlay_opacity);
        if let Some(o) = outline {
            cv.image(&o, tx, ty, f64::from(o.width()) * scale, f64::from(o.height()) * scale, 1.0);
        }
        self.draw_markers(cv, s, l, &p, ms, false);
        cv.pop_layer();
        cv.pop_layer();
        cv.pop_layer();
        self.view_cone(cv, s, c, f.camera, 150.0 * ui);
        self.player(cv, c, f.heading, 46.0 * ui, true);
    }

    /// Round minimap that turns with the player (heading up), north marked on the rim.
    #[allow(clippy::too_many_arguments)]
    fn render_disc(&mut self, cv: &mut Canvas, s: &Settings, f: &OverlayFrame, l: &Layout, me: P2, panel: R, scale: f64, ms: f64, ui: f64) {
        let c = (panel.0 + panel.2 / 2.0, panel.1 + panel.3 / 2.0);
        let r = panel.2 / 2.0;
        let turn = f.heading.map_or(0.0, |h| -h);
        let (tx, ty) = (c.0 - me.0 * scale, c.1 - me.1 * scale);
        let p = |img: &[f64]| (img[0] * scale + tx, img[1] * scale + ty);
        let clip = ellipse(c.0, c.1, r, r);
        cv.push_opacity(self.mini_opacity(s));
        cv.fill(clip.clone(), &Fill::Solid(PANEL_FILL));
        if let Some(cl) = &clip {
            cv.push_clip(cl);
        }
        cv.push_rotate(turn, c.0, c.1);
        self.spin = -turn; // icons and labels stay upright
        if let Some(bmp) = self.art(l, f.floor) {
            let rr = Self::image_rect(l, f.floor, &bmp, tx, ty, scale);
            cv.image(&bmp, rr.0, rr.1, rr.2, rr.3, 0.9);
        }
        self.draw_markers(cv, s, l, &p, ms, false);
        self.spin = 0.0;
        cv.pop_transform();
        // a soft vignette so the edge reads as a lens, not a cut
        cv.fill(ellipse(c.0, c.1, r, r), &Fill::Radial(c.0 as f32, c.1 as f32, r as f32, vec![(0.78, Rgba(8, 10, 18, 0)), (1.0, Rgba(8, 10, 18, 170))]));
        // the map turns with the heading, so the camera's cone turns with it
        self.view_cone(cv, s, c, f.camera.map(|cam| cam + turn), r * 0.45);
        self.player(cv, c, f.heading.map(|_| 0.0), 34.0 * ui, true);
        if clip.is_some() {
            cv.pop_layer();
        }
        cv.stroke(ellipse(c.0, c.1, r, r), Pen::new(Rgba(255, 255, 255, 170), 1.5 * ui));
        // north on the rim, where north now is
        let a = turn.to_radians();
        let n = (c.0 + a.sin() * r, c.1 - a.cos() * r);
        cv.draw(ellipse(n.0, n.1, 10.0 * ui, 10.0 * ui), Some(&Fill::Solid(DISC)), Some(Pen::new(DISC_RING, 1.2)));
        self.label(cv, "N", (n.0, n.1 - 8.0 * ui), Rgba::rgb(0xFF, 0x6B, 0x5E), 12.0 * ui, false);
        self.caption(cv, s, f, l, panel, ui, 0.0);
        cv.pop_layer();
    }

    /// What the radar and compass point at, honouring the layer toggles.
    fn targets(s: &Settings, l: &Layout) -> Vec<Target> {
        let mut t: Vec<Target> = vec![];
        if l.is_island() {
            for m in &l.markers {
                match m.kind.as_str() {
                    "entrance_main" if s.layer("doors") => t.push(("door_orange", m.img.clone(), color("orange"), 1.0)),
                    "entrance_side" if s.layer("doors") => t.push(("door_blue", m.img.clone(), color("blue"), 1.0)),
                    "boat" => t.push(("boat", m.img.clone(), BOAT, 1.1)),
                    "rare" if s.layer("legendary") => t.push(("treasure", m.img.clone(), color("legendary"), 0.9)),
                    _ => {}
                }
            }
            return t;
        }
        if s.layer("doors")
            && l.has_doors()
            && let (Some(b), Some(o)) = (l.blue(), l.orange())
        {
            t.push(("door_blue", b.img.clone(), color("blue"), 1.1));
            t.push(("door_orange", o.img.clone(), color("orange"), 1.0));
        }
        if s.layer("key") {
            for r in l.key_rooms() {
                t.push(("key", r.door.as_ref().map_or_else(|| r.img.clone(), |d| d.img.clone()), key_color(r), 1.2));
            }
        }
        if s.layer("challenge") {
            for r in l.rooms.iter().filter(|r| r.kind == "challenge") {
                t.push(("challenge", r.img.clone(), PALE, 1.0));
            }
        }
        if s.layer("nest") {
            for m in l.markers.iter().filter(|m| m.kind == "nest") {
                t.push(("nest", m.img.clone(), color("nest"), 1.0));
            }
        }
        t
    }

    /// On the island only the nearest of each kind (the Sanctum's few are all kept).
    fn nearest(targets: &[Target], me: P2, island: bool) -> Vec<usize> {
        if !island {
            return (0..targets.len()).collect();
        }
        let mut best: HashMap<&str, usize> = HashMap::new();
        for (i, t) in targets.iter().enumerate() {
            let e = best.entry(t.0).or_insert(i);
            if dist(&t.1, me) < dist(&targets[*e].1, me) {
                *e = i;
            }
        }
        let mut v: Vec<usize> = best.into_values().collect();
        v.sort_unstable();
        v
    }

    /// Radar: a faint outline and what matters; anything out of range on the rim with an arrow and its distance.
    #[allow(clippy::too_many_arguments)]
    fn render_radar(&mut self, cv: &mut Canvas, s: &Settings, f: &OverlayFrame, l: &Layout, me: P2, panel: R, scale: f64, ms: f64, ui: f64) {
        let c = (panel.0 + panel.2 / 2.0, panel.1 + panel.3 / 2.0);
        let r = panel.2 / 2.0;
        let (tx, ty) = (c.0 - me.0 * scale, c.1 - me.1 * scale);
        let clip = ellipse(c.0, c.1, r, r);
        cv.push_opacity(self.mini_opacity(s));
        cv.fill(clip.clone(), &Fill::Solid(RADAR_FILL));
        if let Some(cl) = &clip {
            cv.push_clip(cl);
        }
        if l.is_island() {
            if let Some(bmp) = self.art(l, f.floor) {
                let rr = Self::image_rect(l, f.floor, &bmp, tx, ty, scale);
                cv.image(&bmp, rr.0, rr.1, rr.2, rr.3, 0.3);
            }
        } else if let Some(o) = self.outline(l, f.floor) {
            cv.image(&o, tx, ty, f64::from(o.width()) * scale, f64::from(o.height()) * scale, 0.55);
        }
        // range rings with their distance in metres, and a crosshair
        let mpp = 1.0 / (scale * l.px_per_metre().max(1.0));
        for k in [1.0 / 3.0, 2.0 / 3.0] {
            cv.stroke(ellipse(c.0, c.1, r * k, r * k), Pen::new(RADAR_RING, 1.0));
            self.label(cv, &format!("{:.0} m", r * k * mpp), (c.0, c.1 - r * k - ui), Rgba(180, 230, 255, 150), 9.5 * ui, true);
        }
        cv.stroke(line((c.0 - r, c.1), (c.0 + r, c.1)), Pen::new(RADAR_RING, 1.0));
        cv.stroke(line((c.0, c.1 - r), (c.0, c.1 + r)), Pen::new(RADAR_RING, 1.0));
        if clip.is_some() {
            cv.pop_layer();
        }
        let icon = 36.0 * ms;
        let targets = Self::targets(s, l);
        let rim = Self::nearest(&targets, me, l.is_island());
        for (i, t) in targets.iter().enumerate() {
            let p = (t.1[0] * scale + tx, t.1[1] * scale + ty);
            let (dx, dy) = (p.0 - c.0, p.1 - c.1);
            let d = (dx * dx + dy * dy).sqrt();
            if d <= r - icon * 0.55 {
                self.icon(cv, t.0, p, icon * t.3, 1.0);
                continue;
            }
            if !rim.contains(&i) || d == 0.0 {
                continue;
            }
            // out of range: on the rim, pointing the way, with how far
            let metres = dist(&t.1, me) / l.px_per_metre().max(1.0);
            let (ux, uy) = (dx / d, dy / d);
            let at = (c.0 + ux * (r - icon * 0.45), c.1 + uy * (r - icon * 0.45));
            let tip = (c.0 + ux * (r + 9.0 * ui), c.1 + uy * (r + 9.0 * ui));
            let bl = (c.0 + ux * (r + ui) - uy * 6.0 * ui, c.1 + uy * (r + ui) + ux * 6.0 * ui);
            let br = (c.0 + ux * (r + ui) + uy * 6.0 * ui, c.1 + uy * (r + ui) - ux * 6.0 * ui);
            cv.draw(polygon(&[tip, bl, br]), Some(&Fill::Solid(t.2)), Some(Pen::new(Rgba::BLACK, ui)));
            self.icon(cv, t.0, at, icon * t.3 * 0.8, 0.95);
            self.label(cv, &format!("{metres:.0} m"), (at.0 - ux * icon * 0.55, at.1 - uy * icon * 0.55 + 4.0 * ui), t.2, 10.0 * ui, false);
        }
        self.view_cone(cv, s, c, f.camera, r * 0.45);
        self.player(cv, c, f.heading, 32.0 * ui, true);
        cv.stroke(ellipse(c.0, c.1, r, r), Pen::new(Rgba(120, 210, 255, 170), 1.5 * ui));
        self.label(cv, "N", (c.0, panel.1 + 4.0 * ui), Rgba::WHITE, 12.0 * ui, false);
        self.caption(cv, s, f, l, panel, ui, 0.0);
        cv.pop_layer();
    }

    /// Compass strip: the way you face in the middle, 90 degrees to each side, and where things are with distances.
    #[allow(clippy::too_many_arguments)]
    fn render_compass(&mut self, cv: &mut Canvas, s: &Settings, f: &OverlayFrame, l: &Layout, me: P2, panel: R, ui: f64) {
        let facing = f.heading.unwrap_or(0.0);
        let (cx, half) = (panel.0 + panel.2 / 2.0, panel.2 / 2.0);
        let ppd = half / 90.0;
        let rel = |b: f64| ((b - facing) % 360.0 + 540.0) % 360.0 - 180.0;
        cv.push_opacity(self.mini_opacity(s));
        let (k, clear) = (Rgba(0, 0, 0, 255), Rgba(0, 0, 0, 0));
        let (x0, x1) = (panel.0 as f32, (panel.0 + panel.2) as f32);
        cv.push_opacity_mask(&Fill::Linear((x0, 0.0), (x1, 0.0), vec![(0.0, clear), (0.12, k), (0.88, k), (1.0, clear)]));
        cv.fill(rounded_rect(panel.0, panel.1, panel.2, panel.3, panel.3 / 2.0), &Fill::Solid(PANEL_FILL));
        let bottom = panel.1 + panel.3;
        cv.stroke(line((panel.0, bottom - 1.0), (panel.0 + panel.2, bottom - 1.0)), Pen::new(Rgba(255, 255, 255, 120), ui));
        const NAMES: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
        for deg in (0..360).step_by(15) {
            let r = rel(f64::from(deg));
            if r.abs() > 90.0 {
                continue;
            }
            let x = cx + r * ppd;
            let major = deg % 45 == 0;
            cv.stroke(
                line((x, panel.1 + 4.0 * ui), (x, panel.1 + if major { 13.0 } else { 9.0 } * ui)),
                Pen::new(Rgba(255, 255, 255, if major { 200 } else { 110 }), if major { 2.0 } else { 1.0 } * ui),
            );
            if major {
                let col = if deg == 0 { Rgba::rgb(0xFF, 0x6B, 0x5E) } else { Rgba::WHITE };
                self.label(cv, NAMES[(deg / 45) as usize], (x, panel.1 + 14.0 * ui), col, if deg % 90 == 0 { 13.0 } else { 10.5 } * ui, false);
            }
        }
        cv.pop_layer();
        // the way you face: a small marker in the middle
        let mark = polygon(&[(cx, panel.1 + 9.0 * ui), (cx - 6.0 * ui, panel.1 - ui), (cx + 6.0 * ui, panel.1 - ui)]);
        cv.draw(mark, Some(&Fill::Solid(color("player"))), Some(Pen::new(Rgba::BLACK, ui)));
        let icon = 26.0 * ui;
        let (mut pinned_left, mut pinned_right) = (0, 0);
        let mut rows: [Vec<f64>; 2] = [vec![], vec![]];
        let targets = Self::targets(s, l);
        let all: Vec<(usize, f64, f64)> = Self::nearest(&targets, me, l.is_island())
            .into_iter()
            .map(|i| {
                let t = &targets[i];
                (i, rel((t.1[0] - me.0).atan2(-(t.1[1] - me.1)).to_degrees()), dist(&t.1, me))
            })
            .collect();
        // nearest first for the pinned ones (they take the end slots), far to near otherwise
        let mut behind: Vec<_> = all.iter().filter(|x| x.1.abs() > 90.0).copied().collect();
        behind.sort_by(|a, b| a.2.total_cmp(&b.2));
        let mut ahead: Vec<_> = all.iter().filter(|x| x.1.abs() <= 90.0).copied().collect();
        ahead.sort_by(|a, b| b.2.total_cmp(&a.2));
        for (i, r, _) in behind.into_iter().chain(ahead) {
            let t = &targets[i];
            let is_behind = r.abs() > 90.0;
            let mut x = cx + r.clamp(-90.0, 90.0) * ppd;
            if is_behind {
                let n = if r > 0.0 {
                    pinned_right += 1;
                    pinned_right - 1
                } else {
                    pinned_left += 1;
                    pinned_left - 1
                };
                x -= r.signum() * icon * (0.6 + 1.5 * f64::from(n));
            }
            let p = (x, panel.1 + panel.3 * 0.66);
            let metres = dist(&t.1, me) / l.px_per_metre().max(1.0);
            self.icon(cv, t.0, p, icon * t.3, if is_behind { 0.6 } else { 1.0 });
            if is_behind && (if r > 0.0 { pinned_right } else { pinned_left }) == 1 {
                self.label(cv, if r > 0.0 { "›" } else { "‹" }, (x + r.signum() * icon * 0.75, p.1 - 12.0 * ui), Rgba::WHITE, 18.0 * ui, false);
            }
            // distances in up to two rows under the strip; one that fits neither is left out
            let lw = 34.0 * ui;
            let row = if !rows[0].iter().any(|o| (o - x).abs() < lw) {
                0
            } else if !rows[1].iter().any(|o| (o - x).abs() < lw) {
                1
            } else {
                continue;
            };
            rows[row].push(x);
            self.label(cv, &format!("{metres:.0} m"), (x, bottom + (2.0 + 12.0 * row as f64) * ui), t.2, 10.5 * ui, false);
        }
        let gap = if s.caption_position == "Above" { 0.0 } else { 28.0 * ui };
        self.caption(cv, s, f, l, panel, ui, gap);
        cv.pop_layer();
    }

    /// The line above or below a map panel: which Sanctum and the difficulty, as chosen in the settings.
    #[allow(clippy::too_many_arguments)]
    fn caption(&mut self, cv: &mut Canvas, s: &Settings, f: &OverlayFrame, l: &Layout, panel: R, ui: f64, gap: f64) {
        if s.caption_position == "Off" {
            return;
        }
        let mut parts = vec![];
        if s.caption_layout {
            parts.push(l.display_title());
        }
        if s.caption_difficulty
            && let Some(d) = f.difficulty.as_deref().filter(|d| !d.is_empty())
        {
            parts.push(lang::difficulty(Some(d)));
        }
        if parts.is_empty() {
            return;
        }
        let above = s.caption_position == "Above";
        let at = (panel.0 + panel.2 / 2.0, if above { panel.1 - 4.0 * ui - gap } else { panel.1 + panel.3 + 4.0 * ui + gap });
        self.label(cv, &parts.join("  ·  "), at, difficulty_color(f.difficulty.as_deref()), 13.0 * ui, above);
    }

    /// While placing: a dashed frame with a grip, the hint, and Done / Reset / Cancel.
    fn placement_chrome(&mut self, cv: &mut Canvas, panel: R, style: &str, ui: f64) {
        let accent = Rgba::rgb(0x4D, 0xD8, 0xFF);
        let fr = (panel.0 - 6.0 * ui, panel.1 - 6.0 * ui, panel.2 + 12.0 * ui, panel.3 + 12.0 * ui);
        // a faint fill keeps every pixel of the panel hit-testable, so it can be grabbed anywhere
        cv.draw(rounded_rect(fr.0, fr.1, fr.2, fr.3, 20.0 * ui), Some(&Fill::Solid(accent.with_alpha(28))), Some(Pen::new(accent, 2.0 * ui).dashed()));
        for i in 0..3 {
            let o = (6.0 + f64::from(i) * 5.0) * ui;
            cv.stroke(line((fr.0 + fr.2 - o, fr.1 + fr.3 - 4.0 * ui), (fr.0 + fr.2 - 4.0 * ui, fr.1 + fr.3 - o)), Pen::new(accent, 1.6 * ui));
        }
        let hint = format!(
            "{} · {}",
            lang::t(if style == "Compass" { "Drag to move · scroll to change width" } else { "Drag to move · scroll to resize" }),
            lang::t("Enter = done, Esc = cancel")
        );
        let (tw, th) = text::measure(&hint, 13.0 * ui);
        let (bw, bh, gap) = (84.0 * ui, 30.0 * ui, 8.0 * ui);
        let row_w = (tw + 24.0 * ui).max(3.0 * bw + 2.0 * gap + 24.0 * ui);
        let box_h = th + bh + 26.0 * ui;
        let (cw, ch) = (f64::from(self.placement_client.2.max(1)), f64::from(self.placement_client.3.max(1)));
        let mut top = fr.1 + fr.3 + 30.0 * ui;
        if top + box_h > ch - 8.0 * ui {
            top = fr.1 - box_h - 30.0 * ui; // no room below: above
        }
        let left = (panel.0 + panel.2 / 2.0 - row_w / 2.0).clamp(8.0 * ui, (8.0 * ui).max(cw - row_w - 8.0 * ui));
        cv.draw(rounded_rect(left, top, row_w, box_h, 10.0 * ui), Some(&Fill::Solid(Rgba(14, 17, 28, 235))), Some(Pen::new(Rgba(255, 255, 255, 90), ui)));
        Self::plain_text(cv, &hint, left + (row_w - tw) / 2.0, top + 8.0 * ui, 13.0 * ui, Rgba::WHITE);
        let (by, bx) = (top + th + 16.0 * ui, left + (row_w - (3.0 * bw + 2.0 * gap)) / 2.0);
        self.done_button = Self::button(cv, (bx, by, bw, bh), &lang::t("Done"), accent, true, ui);
        self.reset_button = Self::button(cv, (bx + bw + gap, by, bw, bh), &lang::t("Reset"), accent, false, ui);
        self.cancel_button = Self::button(cv, (bx + 2.0 * (bw + gap), by, bw, bh), &lang::t("Cancel"), accent, false, ui);
    }

    fn button(cv: &mut Canvas, r: R, label: &str, accent: Rgba, primary: bool, ui: f64) -> R {
        cv.fill(rounded_rect(r.0, r.1, r.2, r.3, 7.0 * ui), &Fill::Solid(if primary { accent } else { Rgba::rgb(40, 45, 62) }));
        let (tw, th) = text::measure(label, 13.0 * ui);
        Self::plain_text(cv, label, r.0 + (r.2 - tw) / 2.0, r.1 + (r.3 - th) / 2.0, 13.0 * ui, if primary { Rgba::rgb(8, 20, 30) } else { Rgba::WHITE });
        r
    }

    fn plain_text(cv: &mut Canvas, s: &str, x: f64, y: f64, size: f64, c: Rgba) {
        if let Some(p) = text::path_at(s, size, x, y) {
            cv.text_path(&p, c, None);
        }
    }

    /// Doors, key rooms, the challenge room, nests and loot with the game's own map icons.
    fn draw_markers(&mut self, cv: &mut Canvas, s: &Settings, l: &Layout, p: &dyn Fn(&[f64]) -> P2, ms: f64, labels: bool) {
        let icon = 36.0 * ms;
        let key_rooms: Vec<i64> = l.key_rooms().map(|r| r.id).collect();
        if s.layer("key") {
            for r in l.key_rooms() {
                let kc = key_color(r);
                let key_name = lang::t(match r.door.as_ref().map(|d| d.color.as_str()) {
                    Some("purple") => "Purple key door",
                    Some("blue") => "Blue key door",
                    _ => "Gold key door",
                });
                let (a, b) = match &r.img_rect {
                    Some(q) if q.len() == 4 => (p(&[q[0], q[1]]), p(&[q[2], q[3]])),
                    _ => {
                        // older data: frame the room by its contents (plus ~3 m)
                        let mut inside: Vec<&Vec<f64>> = l.markers.iter().filter(|m| m.room == r.id).map(|m| &m.img).collect();
                        inside.push(&r.img);
                        let pad = 18.0 + if inside.len() == 1 { 30.0 } else { 0.0 };
                        let (mnx, mny) = (inside.iter().map(|q| q[0]).fold(f64::MAX, f64::min), inside.iter().map(|q| q[1]).fold(f64::MAX, f64::min));
                        let (mxx, mxy) = (inside.iter().map(|q| q[0]).fold(f64::MIN, f64::max), inside.iter().map(|q| q[1]).fold(f64::MIN, f64::max));
                        (p(&[mnx - pad, mny - pad]), p(&[mxx + pad, mxy + pad]))
                    }
                };
                let rr = (a.0.min(b.0), a.1.min(b.1), (b.0 - a.0).abs(), (b.1 - a.1).abs());
                cv.draw(rounded_rect(rr.0, rr.1, rr.2, rr.3, 6.0), Some(&Fill::Solid(kc.with_alpha(28))), Some(Pen::new(kc.with_alpha(220), (2.5 * ms).max(2.0)).dashed()));
                let at = r.door.as_ref().map_or(((a.0 + b.0) / 2.0, a.1.min(b.1)), |d| p(&d.img));
                self.icon(cv, "key", at, icon * 1.3, 1.0);
                if labels {
                    self.label(cv, &key_name, (at.0, at.1 - icon * 0.6), kc, 13.0 * ms, true);
                }
            }
        }
        if s.layer("challenge") {
            for r in l.rooms.iter().filter(|r| r.kind == "challenge") {
                let c = p(&r.img);
                self.icon(cv, "challenge", c, icon * 1.05, 1.0);
                if labels {
                    self.label(cv, &lang::term("Challenge", Some("Limited-Time Challenge")), (c.0, c.1 + icon * 0.6), PALE, 12.0 * ms, false);
                }
            }
        }
        // loot, small first so the important ones draw on top
        for kind in ["pile", "pot", "mound", "simple", "exquisite", "luxurious", "legendary", "nest"] {
            if !s.layer(kind) {
                continue;
            }
            for m in l.markers.iter().filter(|m| m.kind == kind) {
                let q = p(&m.img);
                match kind {
                    "nest" => {
                        self.icon(cv, "nest", q, icon * 1.1, 1.0);
                        if labels {
                            self.label(cv, &lang::term("EggNest", Some("Nest")), (q.0, q.1 + icon * 0.55), color("nest"), 12.0 * ms, false);
                        }
                    }
                    "legendary" => {
                        // always there in a key room; elsewhere only a chance
                        let sure = key_rooms.contains(&m.room);
                        self.icon(cv, "chest_gold", q, if sure { icon } else { icon * 0.72 }, if sure { 1.0 } else { 0.8 });
                        if sure && labels {
                            self.label(cv, &lang::t("Legendary"), (q.0, q.1 + icon * 0.5), color("legendary"), 11.0 * ms, false);
                        }
                    }
                    "luxurious" => self.icon(cv, "chest_purple", q, icon * 0.75, 1.0),
                    "exquisite" => self.icon(cv, "chest_blue", q, icon * 0.62, 1.0),
                    "simple" => self.icon(cv, "chest_white", q, icon * 0.52, 1.0),
                    _ => self.icon(cv, "chest", q, icon * 0.45, 0.85),
                }
            }
        }
        if s.layer("doors")
            && l.has_doors()
            && let (Some(o), Some(b)) = (l.orange(), l.blue())
        {
            self.door(cv, "door_orange", p(&o.img), color("orange"), &lang::t("Orange door"), icon, ms);
            self.door(cv, "door_blue", p(&b.img), color("blue"), &lang::t("Blue door"), icon, ms);
        }
        // island marks: Sanctum entrances, the Egg Boat, rare chests, places
        for m in &l.markers {
            let q = p(&m.img);
            match m.kind.as_str() {
                "entrance_main" if s.layer("doors") => {
                    self.icon(cv, "door_orange", q, icon, 1.0);
                    if labels {
                        self.label(cv, &lang::t("Main entrance"), (q.0, q.1 + icon * 0.6), color("orange"), 12.0 * ms, false);
                    }
                }
                "entrance_side" if s.layer("doors") => {
                    self.icon(cv, "door_blue", q, icon, 1.0);
                    if labels {
                        self.label(cv, &lang::t("Side entrance"), (q.0, q.1 + icon * 0.6), color("blue"), 12.0 * ms, false);
                    }
                }
                "boat" => {
                    self.icon(cv, "boat", q, icon * 1.1, 1.0);
                    if labels {
                        self.label(cv, &lang::term("EggBoat", Some("Egg Boat")), (q.0, q.1 + icon * 0.65), BOAT, 13.0 * ms, false);
                    }
                }
                "rare" if s.layer("legendary") => {
                    self.icon(cv, "treasure", q, icon * 0.9, 1.0);
                    if labels {
                        self.label(cv, &lang::t("Rare chest"), (q.0, q.1 + icon * 0.55), color("legendary"), 11.0 * ms, false);
                    }
                }
                "area" if labels => self.label(cv, &m.name, q, Rgba::rgb(0xF2, 0xEE, 0xE0), 14.0 * ms, true),
                _ => {}
            }
        }
    }

    /// An icon centred on c, fitted into a size x size box; icons without their own backing sit on a dark disc.
    fn icon(&mut self, cv: &mut Canvas, name: &str, c: P2, size: f64, opacity: f64) {
        if !cv.near_view(c.0, c.1, size) {
            return;
        }
        let spun = self.spin != 0.0;
        if spun {
            cv.push_rotate(self.spin, c.0, c.1);
        }
        let mut size = size;
        if !name.starts_with("door_") && name != "key" {
            // (faded by its colours' alpha rather than a layer of its own: a whole-canvas layer per faded loot mark
            // was most of a frame)
            let fade = |c: Rgba| c.with_alpha((f64::from(c.3) * opacity).round() as u8);
            cv.draw(ellipse(c.0, c.1, size * 0.56, size * 0.56), Some(&Fill::Solid(fade(DISC))), Some(Pen::new(fade(DISC_RING), 1.2)));
            size *= if name.starts_with("chest_") { 0.9 } else { 0.78 };
        }
        match self.icon_image(name) {
            None => cv.fill(ellipse(c.0, c.1, size / 4.0, size / 4.0), &Fill::Solid(Rgba::WHITE)),
            Some(bmp) => {
                let k = size / f64::from(bmp.width().max(bmp.height()));
                let (w, h) = (f64::from(bmp.width()) * k, f64::from(bmp.height()) * k);
                cv.image(&bmp, c.0 - w / 2.0, c.1 - h / 2.0, w, h, opacity);
            }
        }
        if spun {
            cv.pop_transform();
        }
    }

    /// Where the camera looks: a soft white wedge (80 degrees, fading out with distance).
    fn view_cone(&self, cv: &mut Canvas, s: &Settings, p: P2, camera: Option<f64>, length: f64) {
        let Some(deg) = camera.filter(|_| s.show_view_cone) else { return };
        let half = 40f64.to_radians();
        let a = deg.to_radians();
        let at = |ang: f64| (p.0 + ang.sin() * length, p.1 - ang.cos() * length);
        let mut pts = vec![p];
        for i in 0..=24 {
            pts.push(at(a - half + 2.0 * half * f64::from(i) / 24.0));
        }
        let w = Rgba(255, 255, 255, 0);
        let fill = Fill::Radial(p.0 as f32, p.1 as f32, length as f32, vec![(0.0, w.with_alpha(165)), (0.55, w.with_alpha(95)), (1.0, w)]);
        cv.fill(polygon(&pts), &fill);
    }

    /// The player: the game's own minimap arrow turned the way they face (a plain marker while that isn't known).
    fn player(&mut self, cv: &mut Canvas, p: P2, heading: Option<f64>, size: f64, fresh: bool) {
        if self.egg
            && let Some(egg) = self.icon_image("egg_marker")
        {
            let w = size * 0.95;
            cv.image(&egg, p.0 - w / 2.0, p.1 - w / 2.0, w, w, 1.0);
            return;
        }
        let arrow = self.icon_image("player_arrow");
        match (arrow, heading) {
            (Some(arrow), Some(h)) => {
                cv.push_rotate(h, p.0, p.1);
                cv.image(&arrow, p.0 - size / 2.0, p.1 - size / 2.0, size, size, if fresh { 1.0 } else { 0.55 });
                cv.pop_transform();
            }
            _ => {
                let c = color("player");
                let a = if fresh { 255 } else { 120 };
                cv.fill(ellipse(p.0, p.1, size * 0.6, size * 0.6), &Fill::Solid(c.with_alpha(a / 3)));
                cv.draw(ellipse(p.0, p.1, size * 0.25, size * 0.25), Some(&Fill::Solid(c.with_alpha(a))), Some(Pen::new(Rgba::BLACK, size * 0.08)));
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn door(&mut self, cv: &mut Canvas, icon: &str, p: P2, c: Rgba, label: &str, size: f64, ms: f64) {
        if !cv.near_view(p.0, p.1, size * 2.0 + 13.0 * ms * label.chars().count() as f64) {
            return;
        }
        // a ring around the game's own door icon so it stands out above the map art
        cv.stroke(ellipse(p.0, p.1, size * 0.72, size * 0.72), Pen::new(c.with_alpha(200), 3.0 * ms));
        self.icon(cv, icon, p, size * 1.15, 1.0);
        self.label(cv, label, (p.0, p.1 + size * 0.78), c, 13.0 * ms, false);
    }

    /// A label centred on x, its top at y (its bottom when `above`), with a dark outline.
    fn label(&self, cv: &mut Canvas, s: &str, at: P2, c: Rgba, size: f64, above: bool) {
        let size = size.max(9.0);
        if !cv.near_view(at.0, at.1, size * (s.chars().count() as f64 + 2.0)) {
            return;
        }
        let Some((path, _, _)) = text::label_path(s, size, above) else { return };
        let spun = self.spin != 0.0;
        if spun {
            cv.push_rotate(self.spin, at.0, at.1);
        }
        cv.push_translate(at.0, at.1);
        cv.text_path(&path, c, Some((Rgba(0, 0, 0, 220), (size / 4.0).max(2.5))));
        cv.pop_transform();
        if spun {
            cv.pop_transform();
        }
    }

    fn legend(&mut self, cv: &mut Canvas, f: &OverlayFrame, l: &Layout, ui: f64, aligned: bool) {
        let count = |n: usize, one: &str, many: &str| if n == 1 { lang::t(one) } else { lang::f(many, &[&n]) };
        let first = format!("{}{}", l.display_title(), f.difficulty.as_deref().map_or(String::new(), |d| format!("  ·  {}", lang::difficulty(Some(d)))));
        let second = if l.is_island() {
            lang::f("Sanctum entrances: orange = main, blue = side  ·  {0} to escape", &[&lang::term("EggBoat", Some("Egg Boat"))])
        } else {
            format!(
                "{}  ·  {}  ·  {}",
                lang::f("Blue door to the {0}", &[&lang::t(l.compass())]),
                count(l.key_rooms().count(), "1 key room", "{0} key rooms"),
                count(l.nest_count(), "1 possible nest", "{0} possible nests")
            )
        };
        let mut lines = vec![(first, color("key")), (second, Rgba::WHITE)];
        if !aligned && !self.preview {
            lines.push((lang::t("Not lined up: open the full map (M) and hold the key again"), Rgba::rgb(0xFF, 0xB0, 0x60)));
        }
        let size = 15.0 * ui;
        let mut y = 18.0 * ui;
        let sizes: Vec<(f64, f64)> = lines.iter().map(|l| text::measure(&l.0, size)).collect();
        let w = sizes.iter().map(|s| s.0).fold(0.0, f64::max);
        let h: f64 = sizes.iter().map(|s| s.1 + 3.0 * ui).sum();
        // top centre: the game's map keeps its title top-left and zoom bar on the left
        let x = if self.preview { 22.0 * ui } else { (f64::from(f.client.2) - w) / 2.0 };
        cv.fill(rounded_rect(x - 12.0 * ui, y - 8.0 * ui, w + 24.0 * ui, h + 14.0 * ui, 10.0), &Fill::Solid(Rgba(10, 12, 20, 185)));
        for ((t, c), (_, lh)) in lines.iter().zip(sizes) {
            Self::plain_text(cv, t, x, y, size, *c);
            y += lh + 3.0 * ui;
        }
    }

    fn banner(&mut self, cv: &mut Canvas, f: &OverlayFrame, ui: f64, t: &str) {
        let (tw, th) = text::measure(t, 16.0 * ui);
        let (x, y) = ((f64::from(f.client.2) - tw) / 2.0, 90.0 * ui);
        cv.fill(rounded_rect(x - 16.0 * ui, y - 10.0 * ui, tw + 32.0 * ui, th + 20.0 * ui, 10.0), &Fill::Solid(Rgba(10, 12, 20, 200)));
        Self::plain_text(cv, t, x, y, 16.0 * ui, Rgba::WHITE);
    }
}
