//! A small retained-state drawing surface over tiny-skia, shaped like WPF's DrawingContext as the overlay used it:
//! a transform stack and layers (opacity, a clip, an opacity mask) that are composited when popped.

use tiny_skia::{
    BlendMode, Color, FillRule, FilterQuality, GradientStop, LineJoin, LinearGradient, Mask, MaskType, Paint, Path, PathBuilder, Pattern, Pixmap,
    PixmapPaint, Point, RadialGradient, Rect, Shader, SpreadMode, Stroke, StrokeDash, Transform,
};

/// An RGBA colour (straight alpha), as WPF's Color.FromArgb.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    pub const WHITE: Rgba = Rgba(255, 255, 255, 255);
    pub const BLACK: Rgba = Rgba(0, 0, 0, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Rgba {
        Rgba(r, g, b, 255)
    }

    pub fn with_alpha(self, a: u8) -> Rgba {
        Rgba(self.0, self.1, self.2, a)
    }

    fn color(self) -> Color {
        Color::from_rgba8(self.0, self.1, self.2, self.3)
    }
}

/// What fills a shape.
#[derive(Clone, Debug)]
pub enum Fill {
    Solid(Rgba),
    /// Centre, radius, stops (offset 0-1, colour).
    Radial(f32, f32, f32, Vec<(f32, Rgba)>),
    /// Start, end, stops.
    Linear((f32, f32), (f32, f32), Vec<(f32, Rgba)>),
}

fn stops(s: &[(f32, Rgba)]) -> Vec<GradientStop> {
    s.iter().map(|(o, c)| GradientStop::new(*o, c.color())).collect()
}

impl Fill {
    fn shader(&self) -> Option<Shader<'static>> {
        match self {
            Fill::Solid(c) => Some(Shader::SolidColor(c.color())),
            Fill::Radial(x, y, r, s) => RadialGradient::new(Point::from_xy(*x, *y), Point::from_xy(*x, *y), *r, stops(s), SpreadMode::Pad, Transform::identity()),
            Fill::Linear(a, b, s) => LinearGradient::new(Point::from_xy(a.0, a.1), Point::from_xy(b.0, b.1), stops(s), SpreadMode::Pad, Transform::identity()),
        }
    }
}

/// A pen: colour, width, dashed (WPF's DashStyles.Dash: 2 on, 2 off, in widths).
#[derive(Clone, Copy, Debug)]
pub struct Pen {
    pub color: Rgba,
    pub width: f32,
    pub dashed: bool,
}

impl Pen {
    pub fn new(color: Rgba, width: f64) -> Pen {
        Pen { color, width: width as f32, dashed: false }
    }

    pub fn dashed(mut self) -> Pen {
        self.dashed = true;
        self
    }
}

struct Layer {
    pix: Pixmap,
    opacity: f32,
    clip: Option<Mask>,
    mask: Option<Mask>,
}

pub struct Canvas {
    layers: Vec<Layer>,
    transforms: Vec<Transform>,
    w: u32,
    h: u32,
}

pub fn rounded_rect(x: f64, y: f64, w: f64, h: f64, r: f64) -> Option<Path> {
    let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);
    let r = (r as f32).min(w / 2.0).min(h / 2.0).max(0.0);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    if r <= 0.0 {
        return Some(PathBuilder::from_rect(Rect::from_xywh(x, y, w, h)?));
    }
    // cubic quarter circles (k = 0.5523)
    let k = 0.552_284_8 * r;
    let mut p = PathBuilder::new();
    p.move_to(x + r, y);
    p.line_to(x + w - r, y);
    p.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    p.line_to(x + w, y + h - r);
    p.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    p.line_to(x + r, y + h);
    p.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    p.line_to(x, y + r);
    p.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    p.close();
    p.finish()
}

pub fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64) -> Option<Path> {
    if rx <= 0.0 || ry <= 0.0 {
        return None;
    }
    PathBuilder::from_oval(Rect::from_xywh((cx - rx) as f32, (cy - ry) as f32, (2.0 * rx) as f32, (2.0 * ry) as f32)?)
}

pub fn polygon(pts: &[(f64, f64)]) -> Option<Path> {
    let mut p = PathBuilder::new();
    let (first, rest) = pts.split_first()?;
    p.move_to(first.0 as f32, first.1 as f32);
    for q in rest {
        p.line_to(q.0 as f32, q.1 as f32);
    }
    p.close();
    p.finish()
}

pub fn line(a: (f64, f64), b: (f64, f64)) -> Option<Path> {
    let mut p = PathBuilder::new();
    p.move_to(a.0 as f32, a.1 as f32);
    p.line_to(b.0 as f32, b.1 as f32);
    p.finish()
}

impl Canvas {
    pub fn new(w: u32, h: u32) -> Option<Canvas> {
        let pix = Pixmap::new(w.max(1), h.max(1))?;
        Some(Canvas { layers: vec![Layer { pix, opacity: 1.0, clip: None, mask: None }], transforms: vec![Transform::identity()], w: w.max(1), h: h.max(1) })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.w, self.h)
    }

    fn t(&self) -> Transform {
        *self.transforms.last().unwrap()
    }

    fn top(&mut self) -> &mut Pixmap {
        &mut self.layers.last_mut().unwrap().pix
    }

    /// Later drawing goes through this transform too (pre-applied, as WPF's PushTransform).
    pub fn push_transform(&mut self, t: Transform) {
        let cur = self.t();
        self.transforms.push(cur.pre_concat(t));
    }

    pub fn push_translate(&mut self, x: f64, y: f64) {
        self.push_transform(Transform::from_translate(x as f32, y as f32));
    }

    /// Degrees clockwise around (cx, cy), as WPF's RotateTransform.
    pub fn push_rotate(&mut self, deg: f64, cx: f64, cy: f64) {
        self.push_transform(Transform::from_rotate_at(deg as f32, cx as f32, cy as f32));
    }

    pub fn push_scale(&mut self, sx: f64, sy: f64) {
        self.push_transform(Transform::from_scale(sx as f32, sy as f32));
    }

    /// Could something drawn within `margin` of (x, y) (in the current transform) show on the canvas?
    pub fn near_view(&self, x: f64, y: f64, margin: f64) -> bool {
        let t = self.t();
        let mut p = [Point::from_xy(x as f32, y as f32)];
        t.map_points(&mut p);
        let k = f64::from(t.sx.abs().max(t.ky.abs()).max(t.kx.abs()).max(t.sy.abs()).max(1e-6));
        let m = (margin * k) as f32;
        p[0].x >= -m && p[0].y >= -m && p[0].x <= self.w as f32 + m && p[0].y <= self.h as f32 + m
    }

    pub fn pop_transform(&mut self) {
        if self.transforms.len() > 1 {
            self.transforms.pop();
        }
    }

    fn new_layer(&mut self, opacity: f64, clip: Option<Mask>, mask: Option<Mask>) {
        let pix = Pixmap::new(self.w, self.h).unwrap();
        self.layers.push(Layer { pix, opacity: opacity.clamp(0.0, 1.0) as f32, clip, mask });
    }

    /// Everything until `pop_layer` is drawn at this opacity.
    pub fn push_opacity(&mut self, opacity: f64) {
        self.new_layer(opacity, None, None);
    }

    /// Everything until `pop_layer` is clipped to this shape (in the current transform).
    pub fn push_clip(&mut self, shape: &Path) {
        let mut m = Mask::new(self.w, self.h).unwrap();
        m.fill_path(shape, FillRule::Winding, true, self.t());
        self.new_layer(1.0, Some(m), None);
    }

    /// Everything until `pop_layer` takes its alpha from this fill's alpha (WPF's PushOpacityMask).
    pub fn push_opacity_mask(&mut self, fill: &Fill) {
        let mut p = Pixmap::new(self.w, self.h).unwrap();
        if let Some(shader) = fill.shader() {
            let paint = Paint { shader, anti_alias: true, ..Default::default() };
            p.fill_rect(Rect::from_xywh(0.0, 0.0, self.w as f32, self.h as f32).unwrap(), &paint, self.t(), None);
        }
        let m = Mask::from_pixmap(p.as_ref(), MaskType::Alpha);
        self.new_layer(1.0, None, Some(m));
    }

    pub fn pop_layer(&mut self) {
        if self.layers.len() < 2 {
            return;
        }
        let mut l = self.layers.pop().unwrap();
        if let Some(c) = &l.clip {
            l.pix.apply_mask(c);
        }
        if let Some(m) = &l.mask {
            l.pix.apply_mask(m);
        }
        let paint = PixmapPaint { opacity: l.opacity, blend_mode: BlendMode::SourceOver, quality: FilterQuality::Nearest };
        self.top().draw_pixmap(0, 0, l.pix.as_ref(), &paint, Transform::identity(), None);
    }

    pub fn fill(&mut self, shape: Option<Path>, fill: &Fill) {
        let (Some(shape), Some(shader)) = (shape, fill.shader()) else { return };
        let t = self.t();
        let paint = Paint { shader, anti_alias: true, ..Default::default() };
        self.top().fill_path(&shape, &paint, FillRule::Winding, t, None);
    }

    pub fn stroke(&mut self, shape: Option<Path>, pen: Pen) {
        let Some(shape) = shape else { return };
        let t = self.t();
        let paint = Paint { shader: Shader::SolidColor(pen.color.color()), anti_alias: true, ..Default::default() };
        let mut s = Stroke { width: pen.width.max(0.01), ..Default::default() };
        if pen.dashed {
            s.dash = StrokeDash::new(vec![2.0 * pen.width, 2.0 * pen.width], 0.0);
        }
        self.top().stroke_path(&shape, &paint, &s, t, None);
    }

    /// Fill and outline, as WPF's DrawGeometry / DrawEllipse / DrawRoundedRectangle with a brush and a pen.
    pub fn draw(&mut self, shape: Option<Path>, fill: Option<&Fill>, pen: Option<Pen>) {
        if let Some(f) = fill {
            self.fill(shape.clone(), f);
        }
        if let Some(p) = pen {
            self.stroke(shape, p);
        }
    }

    /// Text glyphs (as a path, already placed) filled, with an outline under them.
    pub fn text_path(&mut self, shape: &Path, color: Rgba, outline: Option<(Rgba, f64)>) {
        let t = self.t();
        if let Some((oc, w)) = outline {
            let paint = Paint { shader: Shader::SolidColor(oc.color()), anti_alias: true, ..Default::default() };
            let s = Stroke { width: w as f32, line_join: LineJoin::Round, ..Default::default() };
            self.top().stroke_path(shape, &paint, &s, t, None);
        }
        let paint = Paint { shader: Shader::SolidColor(color.color()), anti_alias: true, ..Default::default() };
        self.top().fill_path(shape, &paint, FillRule::Winding, t, None);
    }

    /// A picture stretched into (x, y, w, h) in the current transform.
    pub fn image(&mut self, img: &Pixmap, x: f64, y: f64, w: f64, h: f64, opacity: f64) {
        if w <= 0.0 || h <= 0.0 || img.width() == 0 {
            return;
        }
        let sx = (w / f64::from(img.width())) as f32;
        let sy = (h / f64::from(img.height())) as f32;
        let it = Transform::from_row(sx, 0.0, 0.0, sy, x as f32, y as f32);
        // WPF scales pictures linearly by default
        let quality = FilterQuality::Bilinear;
        let pattern = Pattern::new(img.as_ref(), SpreadMode::Pad, quality, opacity as f32, it);
        let paint = Paint { shader: pattern, anti_alias: false, ..Default::default() };
        let t = self.t();
        if let Some(r) = Rect::from_xywh(x as f32, y as f32, w as f32, h as f32) {
            self.top().fill_rect(r, &paint, t, None);
        }
    }

    /// The finished picture (all layers popped).
    pub fn finish(mut self) -> Pixmap {
        while self.layers.len() > 1 {
            self.pop_layer();
        }
        self.layers.pop().unwrap().pix
    }
}
