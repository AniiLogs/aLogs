//! ImageDraw's rectangle and ellipse (Draw.c: `ImagingDrawRectangle`, `ellipseNew`) and `ImageDraw.floodfill`.

use crate::image::{Image, Mode};

fn ink_px(im: &Image, ink: &[u8]) -> Vec<u8> {
    // one value for a multi-band image is a packed colour, as in Pillow: its first band, the others 0
    let b = im.bands();
    let mut px = vec![0u8; b];
    for (i, v) in px.iter_mut().enumerate() {
        *v = *ink.get(i).unwrap_or(&0);
    }
    if im.mode == Mode::One {
        px[0] = if px[0] != 0 { 255 } else { 0 };
    }
    if im.mode == Mode::Rgba && ink.len() == 3 {
        px[3] = 255;
    }
    px
}

fn hline(im: &mut Image, x0: i64, y: i64, x1: i64, px: &[u8]) {
    if y < 0 || y >= im.height as i64 {
        return;
    }
    let x0 = x0.max(0);
    if x0 >= im.width as i64 || x1 < 0 {
        return;
    }
    let x1 = x1.min(im.width as i64 - 1);
    for x in x0..=x1 {
        im.px_mut(x as usize, y as usize).copy_from_slice(px);
    }
}

/// `ImageDraw.Draw(im).rectangle((x0, y0, x1, y1), fill=ink)`: coordinates truncated to ints, both ends included.
pub fn rectangle(im: &mut Image, xy: (f64, f64, f64, f64), fill: &[u8]) {
    assert!(xy.2 >= xy.0 && xy.3 >= xy.1, "x1 must be greater than or equal to x0");
    let (x0, mut y0, x1, mut y1) = (xy.0 as i64, xy.1 as i64, xy.2 as i64, xy.3 as i64);
    if y0 > y1 {
        std::mem::swap(&mut y0, &mut y1);
    }
    let px = ink_px(im, fill);
    if y0 >= im.height as i64 || y1 < 0 {
        return;
    }
    let y0 = y0.max(0);
    let y1 = y1.min(im.height as i64);
    for y in y0..=y1 {
        hline(im, x0, y, x1, &px);
    }
}

struct Quarter {
    cx: i32,
    cy: i32,
    ex: i32,
    ey: i32,
    a2: i64,
    b2: i64,
    a2b2: i64,
    finished: bool,
}

impl Quarter {
    fn new(a: i32, b: i32) -> Quarter {
        if a < 0 || b < 0 {
            return Quarter { cx: 0, cy: 0, ex: 0, ey: 0, a2: 0, b2: 0, a2b2: 0, finished: true };
        }
        let (a2, b2) = (i64::from(a) * i64::from(a), i64::from(b) * i64::from(b));
        Quarter { cx: a, cy: b % 2, ex: a % 2, ey: b, a2, b2, a2b2: a2 * b2, finished: false }
    }
    fn delta(&self, x: i64, y: i64) -> i64 {
        (self.a2 * y * y + self.b2 * x * x - self.a2b2).abs()
    }
    fn next(&mut self) -> Option<(i32, i32)> {
        if self.finished {
            return None;
        }
        let ret = (self.cx, self.cy);
        if self.cx == self.ex && self.cy == self.ey {
            self.finished = true;
        } else {
            let (mut nx, mut ny) = (self.cx, self.cy + 2);
            let mut nd = self.delta(i64::from(nx), i64::from(ny));
            if nx > 1 {
                let d = self.delta(i64::from(self.cx - 2), i64::from(self.cy + 2));
                if nd > d {
                    nx = self.cx - 2;
                    ny = self.cy + 2;
                    nd = d;
                }
                let d = self.delta(i64::from(self.cx - 2), i64::from(self.cy));
                if nd > d {
                    nx = self.cx - 2;
                    ny = self.cy;
                }
            }
            self.cx = nx;
            self.cy = ny;
        }
        Some(ret)
    }
}

/// `ellipseNew`: the spans `(x0, y, x1)` of an ellipse `a` x `b` (doubled coordinates) with outline width `w`.
fn ellipse_spans(a: i32, b: i32, w: i32) -> Vec<(i32, i32, i32)> {
    let mut out = Vec::new();
    let leftmost = a % 2;
    let mut so = Quarter::new(a, b);
    let Some((pr0, py0)) = (if w < 1 { None } else { so.next() }) else { return out };
    let mut si = Quarter::new(a - 2 * (w - 1), b - 2 * (w - 1));
    let (mut py, mut pl, mut pr) = (py0, leftmost, pr0);
    let mut finished = false;
    loop {
        if finished {
            break;
        }
        let (y, mut l, r) = (py, pl, pr);
        let mut next = so.next();
        while let Some((_, cy)) = next {
            if cy > y {
                break;
            }
            next = so.next();
        }
        match next {
            None => finished = true,
            Some((cx, cy)) => {
                pr = cx;
                py = cy;
            }
        }
        let mut inext = si.next();
        while let Some((cx, cy)) = inext {
            if cy > y {
                break;
            }
            l = cx;
            inext = si.next();
        }
        pl = match inext {
            None => leftmost,
            Some((cx, _)) => cx,
        };
        // pushed in this order, then taken from the end (a stack of up to four)
        let mut buf = Vec::with_capacity(4);
        if (l > 0 || l < r) && y > 0 {
            buf.push((if l == 0 { 2 } else { l }, y, r));
        }
        if y > 0 {
            buf.push((-r, y, -l));
        }
        if l > 0 || l < r {
            buf.push((if l == 0 { 2 } else { l }, -y, r));
        }
        buf.push((-r, -y, -l));
        while let Some(s) = buf.pop() {
            out.push(s);
        }
    }
    out
}

/// `ImageDraw.Draw(im).ellipse((x0, y0, x1, y1), fill=..., outline=..., width=...)`.
pub fn ellipse(im: &mut Image, xy: (f64, f64, f64, f64), fill: Option<&[u8]>, outline: Option<&[u8]>, width: i32) {
    assert!(xy.2 >= xy.0 && xy.3 >= xy.1, "x1 must be greater than or equal to x0");
    let (x0, y0, x1, y1) = (xy.0 as i32, xy.1 as i32, xy.2 as i32, xy.3 as i32);
    let (a, b) = (x1 - x0, y1 - y0);
    let mut draw = |ink: &[u8], w: i32| {
        if a < 0 || b < 0 {
            return;
        }
        let px = ink_px(im, ink);
        for (sx0, sy, sx1) in ellipse_spans(a, b, w) {
            hline(
                im,
                i64::from(x0 + (sx0 + a) / 2),
                i64::from(y0 + (sy + b) / 2),
                i64::from(x0 + (sx1 + a) / 2),
                &px,
            );
        }
    };
    if let Some(f) = fill {
        draw(f, a + b);
    }
    if let Some(o) = outline
        && Some(o) != fill
        && width != 0
    {
        draw(o, width);
    }
}

/// `ImageDraw.floodfill(image, xy, value)` (4-connected, exact colour, no border).
pub fn floodfill(im: &mut Image, xy: (usize, usize), value: &[u8]) {
    let (x, y) = xy;
    if x >= im.width || y >= im.height {
        return;
    }
    let px = ink_px(im, value);
    let background = im.px(x, y).to_vec();
    if background == px {
        return;
    }
    let mut stack = vec![(x, y)];
    im.px_mut(x, y).copy_from_slice(&px);
    while let Some((x, y)) = stack.pop() {
        let try_px = |s: usize, t: usize, im: &mut Image, stack: &mut Vec<(usize, usize)>| {
            if s < im.width && t < im.height && im.px(s, t) == background.as_slice() {
                im.px_mut(s, t).copy_from_slice(&px);
                stack.push((s, t));
            }
        };
        try_px(x + 1, y, im, &mut stack);
        if x > 0 {
            try_px(x - 1, y, im, &mut stack);
        }
        try_px(x, y + 1, im, &mut stack);
        if y > 0 {
            try_px(x, y - 1, im, &mut stack);
        }
    }
}

#[derive(Clone, Copy)]
struct Edge {
    x0: i32,
    y0: i32,
    xmin: i32,
    ymin: i32,
    xmax: i32,
    ymax: i32,
    dx: f32,
}

fn add_edge(x0: i32, y0: i32, x1: i32, y1: i32) -> Edge {
    let dx = if y0 == y1 { 0.0 } else { (x1 - x0) as f32 / (y1 - y0) as f32 };
    Edge { x0, y0, xmin: x0.min(x1), xmax: x0.max(x1), ymin: y0.min(y1), ymax: y0.max(y1), dx }
}

/// `ROUND_UP` / `ROUND_DOWN` (Draw.c) on a float.
fn round_up(f: f32) -> i64 {
    (if f >= 0.0 { ((f + 0.5) as f64).floor() } else { -((f.abs() + 0.5) as f64).floor() }) as i64
}
fn round_down(f: f32) -> i64 {
    (if f >= 0.0 { ((f - 0.5) as f64).ceil() } else { -((f.abs() - 0.5) as f64).ceil() }) as i64
}

/// `ImageDraw.Draw(im).polygon(xy, fill=ink)` (no outline): the vertices truncated to ints, then Draw.c's
/// `ImagingDrawPolygon` fill and `polygon_generic` scan lines, float for float.
pub fn polygon(im: &mut Image, xy: &[(f64, f64)], fill: &[u8]) {
    let count = xy.len();
    if count == 0 {
        return;
    }
    let px = ink_px(im, fill);
    let p: Vec<(i32, i32)> = xy.iter().map(|&(x, y)| (x as i32, y as i32)).collect();
    let mut e: Vec<Edge> = Vec::with_capacity(count);
    let mut i = 0;
    while i < count - 1 {
        let ((x0, y0), (x1, y1)) = (p[i], p[i + 1]);
        if y0 == y1 && i != 0 && y0 == p[i - 1].1 {
            let last = e.len() - 1;
            if x1 > x0 && x0 > p[i - 1].0 {
                e[last].xmax = x1;
                i += 1;
                continue;
            } else if x1 < x0 && x0 < p[i - 1].0 {
                e[last].xmin = x1;
                i += 1;
                continue;
            }
        }
        e.push(add_edge(x0, y0, x1, y1));
        i += 1;
    }
    if p[i] != p[0] {
        e.push(add_edge(p[i].0, p[i].1, p[0].0, p[0].1));
    }
    // polygon_generic
    let (mut ymin, mut ymax) = (im.height as i32 - 1, 0i32);
    let mut table: Vec<Edge> = Vec::new();
    for ed in &e {
        ymin = ymin.min(ed.ymin);
        ymax = ymax.max(ed.ymax);
        if ed.ymin == ed.ymax {
            hline(im, ed.xmin as i64, ed.ymin as i64, ed.xmax as i64, &px);
            continue;
        }
        table.push(*ed);
    }
    ymin = ymin.max(0);
    ymax = ymax.min(im.height as i32);
    let at = |ed: &Edge, y: i32| (y - ed.y0) as f32 * ed.dx + ed.x0 as f32;
    let mut xx: Vec<f32> = Vec::with_capacity(table.len() * 2);
    for y in ymin..=ymax {
        xx.clear();
        for (i, cur) in table.iter().enumerate() {
            if y < cur.ymin || y > cur.ymax {
                continue;
            }
            xx.push(at(cur, y));
            if y == cur.ymax && y < ymax {
                // needed to draw consistent polygons
                xx.push(*xx.last().unwrap());
            } else if (y == cur.ymin || y == cur.ymax) && cur.dx != 0.0 {
                // connect discontiguous corners
                for other in &table[..i] {
                    if (y != other.ymin && y != other.ymax) || other.dx == 0.0 {
                        continue;
                    }
                    let last = *xx.last().unwrap();
                    if last.round() == at(other, y).round() {
                        let offset = if y == cur.ymax { -1 } else { 1 };
                        let adj = at(cur, y + offset);
                        if y + offset >= other.ymin && y + offset <= other.ymax {
                            let adj_other = at(other, y + offset);
                            let n = xx.len() - 1;
                            if last > adj + 1.0 && last > adj_other + 1.0 {
                                xx[n] = ((adj as f64).max(adj_other as f64) as f32).round() + 1.0;
                            } else if last < adj - 1.0 && last < adj_other - 1.0 {
                                xx[n] = ((adj as f64).min(adj_other as f64) as f32).round() - 1.0;
                            }
                            break;
                        }
                    }
                }
            }
        }
        xx.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for k in (1..xx.len()).step_by(2) {
            hline(im, round_up(xx[k - 1]), y as i64, round_down(xx[k]), &px);
        }
    }
}
