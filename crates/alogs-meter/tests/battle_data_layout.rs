//! Battle Data's two layouts (issue 144): with a controller the game draws a button prompt under the cards and has
//! them 38 px higher at 2160, nothing moved sideways. Cards at rest in either layout are found and in place, read in
//! the layout the picture shows; cards anywhere else (on their way in) are not in place. Drawn cards, no game font.

use alogs_meter::meter::Meter;
use alogs_meter::results::BD_LAYOUTS;
use alogs_pil::draw::rectangle;
use alogs_pil::{Filter, Image, Mode};

const W: f64 = 3840.0;
const H: f64 = 2160.0;
/// The controller layout's shift at 2160, in pixels (bottom edge 1575 instead of 1613).
const UP: f64 = 38.0;

/// n cards on a dark scene: where the usual layout has them at rest, moved by (dx, dy) pixels, `w` their width as a
/// share of the screen's. `text`: a dark line across each card where the usual layout's light line is (in the
/// controller layout the game's "Damage Taken" text crosses it). A name tab with a white bar on each.
fn screen(n: usize, dx: f64, dy: f64, w: f64, text: bool) -> Image {
    let mut im = Image::new(Mode::Rgb, W as usize, H as usize, &[40, 55, 90]);
    let bottom = H * 0.7468 + dy;
    for i in 0..n {
        let c = W * (0.5 + (i as f64 - (n as f64 - 1.0) / 2.0) * 0.1943) + dx;
        rectangle(&mut im, (c - W * w / 2.0, bottom - H * 0.46, c + W * w / 2.0, bottom), &[225, 228, 238]);
        rectangle(&mut im, (c - W * 0.04, H * 0.514 + dy, c + W * 0.04, H * 0.545 + dy), &[60, 62, 70]);
        rectangle(&mut im, (c - W * 0.025 + i as f64 * 9.0, H * 0.522 + dy, c + W * 0.02, H * 0.537 + dy), &[250, 250, 250]);
        if text {
            rectangle(&mut im, (c - W * 0.07, H * 0.645 - 6.0, c + W * 0.05, H * 0.645 + 6.0), &[70, 70, 80]);
        }
    }
    im
}

fn usual(n: usize) -> Image {
    screen(n, 0.0, 0.0, 0.1663, false)
}

fn controller(n: usize, text: bool) -> Image {
    screen(n, 0.0, -UP, 0.1663, text)
}

fn scaled(im: &Image, h: usize) -> Image {
    im.resize(h * 16 / 9, h, Some(Filter::Lanczos))
}

fn geo(im: &Image) -> (f64, f64, f64) {
    let h = im.height as f64;
    let u = (im.width as f64).min(h * 16.0 / 9.0);
    (u, (im.width as f64 - u) / 2.0, h)
}

fn count(im: &Image) -> (usize, f64) {
    let (u, ux, h) = geo(im);
    Meter::bd_card_count(im, u, ux, h)
}

fn place(im: &Image, n: usize) -> Option<f64> {
    let (u, ux, h) = geo(im);
    Meter::bd_cards_in_place(im, u, ux, h, n)
}

#[test]
fn the_two_layouts_are_the_usual_one_and_38_px_higher() {
    assert_eq!(BD_LAYOUTS[0], 0.0);
    assert!((BD_LAYOUTS[1] * H + UP).abs() < 0.5, "{}", BD_LAYOUTS[1] * H);
}

#[test]
fn cards_at_rest_in_either_layout_are_counted_and_in_place_in_it() {
    let (c, u) = (BD_LAYOUTS[1], 0.0);
    for size in [2160, 720] {
        let fit = |im: Image| if size == 2160 { im } else { scaled(&im, size) };
        assert_eq!(count(&fit(usual(4))), (4, u), "usual, {size}p");
        assert_eq!(place(&fit(usual(4)), 4), Some(u), "usual, {size}p");
        // the usual line lies on the higher cards too: counted there, but their bottom edge says which layout
        assert_eq!(count(&fit(controller(4, false))), (4, c), "controller, {size}p");
        assert_eq!(place(&fit(controller(4, false)), 4), Some(c), "controller, {size}p");
        // with text across the usual line (as in the game) they are counted on the controller layout's own line
        assert_eq!(count(&fit(controller(4, true))), (4, c), "controller with text, {size}p");
        assert_eq!(place(&fit(controller(4, true)), 4), Some(c), "controller with text, {size}p");
    }
    for n in 1..=3 {
        assert_eq!(count(&usual(n)), (n, u), "{n} cards");
        assert_eq!(place(&usual(n), n), Some(u), "{n} cards");
        assert_eq!(count(&controller(n, true)), (n, c), "{n} cards, controller");
        assert_eq!(place(&controller(n, true), n), Some(c), "{n} cards, controller");
    }
}

#[test]
fn cards_anywhere_else_are_not_in_place() {
    // on their way in: below, between the two resting places, above both, sideways, or not yet full width
    for (what, im) in [
        ("20 px down", screen(4, 0.0, 20.0, 0.1663, false)),
        ("20 px up (between the layouts)", screen(4, 0.0, -20.0, 0.1663, false)),
        ("60 px up (above both)", screen(4, 0.0, -60.0, 0.1663, false)),
        ("60 px up with text", screen(4, 0.0, -60.0, 0.1663, true)),
        ("60 px sideways", screen(4, 60.0, 0.0, 0.1663, false)),
        ("60 px sideways, controller", screen(4, 60.0, -UP, 0.1663, true)),
        ("narrower (zooming in), usual", screen(4, 0.0, 0.0, 0.153, false)),
        ("narrower (zooming in), controller", screen(4, 0.0, -UP, 0.153, true)),
    ] {
        assert_eq!(place(&im, 4), None, "{what}");
    }
    assert_eq!(place(&scaled(&screen(4, 0.0, -20.0, 0.1663, false), 720), 4), None, "between the layouts, 720p");
    assert_eq!(place(&usual(3), 4), None, "a card short");
    assert_eq!(place(&controller(3, true), 4), None, "a card short, controller");
    assert_eq!(place(&usual(4), 0), None, "none");
    let dark = Image::new(Mode::Rgb, W as usize, H as usize, &[40, 55, 90]);
    assert_eq!(count(&dark), (0, 0.0));
    assert_eq!(place(&dark, 4), None);
}

#[test]
fn name_tabs_are_read_where_the_layout_has_them() {
    let (a, b) = (usual(4), controller(4, true));
    let (u, ux, h) = geo(&a);
    let (pa, ga) = Meter::bd_names(&a, 4, u, ux, h, BD_LAYOUTS[0]);
    let (pb, gb) = Meter::bd_names(&b, 4, u, ux, h, BD_LAYOUTS[1]);
    assert_eq!(pa.len(), 4);
    for k in pa.keys() {
        assert_eq!(ga[k].tobytes(), gb[k].tobytes(), "tab {k}: the same crop in both layouts");
        assert_eq!(pa[k].tobytes(), pb[k].tobytes(), "tab {k}: prepared alike");
        assert!(ga[k].convert(Mode::L).getextrema()[0].1 >= 240, "tab {k}: its white bar is in the crop");
    }
    // read in the usual place, the controller layout's tabs are missed
    let (_, miss) = Meter::bd_names(&b, 4, u, ux, h, BD_LAYOUTS[0]);
    assert_ne!(miss["0"].tobytes(), ga["0"].tobytes());
}
