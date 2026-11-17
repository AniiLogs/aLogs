//! The fill of a boss bar read past what is drawn on it: the game's small orange marks on the track (at about 3% and
//! 8% of it) are not the end of the fill. Read as that, a fight showed 6% for 10.7 s while its bar stood at 55%.

use alogs_pil::{Image, Mode};
use alogs_reader::bar::{Bar, COVER, TRACK, WORLD_TRACK_RIGHT, measure_fill_any};

const FILL: [u8; 3] = [250, 107, 105];
const TRACK_DARK: [u8; 3] = [48, 27, 24];
const MARK: [u8; 3] = [255, 176, 64];
const TRAIL: [u8; 3] = [190, 80, 78]; // the lighter trail a hit leaves past the fill

/// A world boss bar as the finder gives it for a 3840-wide frame (its real place and size there).
fn bar() -> Bar {
    let (lb, span) = (1226.0_f64, 1388.0_f64);
    Bar {
        yc: 194,
        left_bracket: lb,
        span,
        brackets_found: true,
        world: true,
        right_red: 0.0,
        track_left: (lb + TRACK.0 * span).round_ties_even() as i64,
        track_right: (lb + WORLD_TRACK_RIGHT * span).round_ties_even() as i64,
        thickness: 26,
        frame: (3840, 400),
    }
}

fn paint(im: &mut Image, x0: i64, x1: i64, colour: [u8; 3]) {
    for y in 181..208 {
        for x in x0.max(0)..x1.min(im.width as i64) {
            let at = (y * im.width + x as usize) * 3;
            im.data[at..at + 3].copy_from_slice(&colour);
        }
    }
}

/// The bar's track filled to `share` of it, the rest empty.
fn frame(b: &Bar, share: f64) -> Image {
    let mut im = Image::new(Mode::Rgb, 3840, 400, &[20, 22, 30]);
    paint(&mut im, b.track_left, b.track_right, TRACK_DARK);
    paint(&mut im, b.track_left, b.track_left + ((b.track_right - b.track_left) as f64 * share) as i64, FILL);
    im
}

fn at(b: &Bar, share: f64) -> i64 {
    b.track_left + ((b.track_right - b.track_left) as f64 * share) as i64
}

fn read(im: &Image, b: &Bar) -> f64 {
    measure_fill_any(im, b, 70).0
}

#[test]
fn a_mark_on_the_bar_is_not_the_end_of_the_fill() {
    let b = bar();
    for share in [0.55, 0.99, 0.20] {
        let plain = frame(&b, share);
        let clean = read(&plain, &b);
        assert!((clean - share).abs() < 0.004, "a bar with nothing on it: {clean} for {share}");
        // the game's two marks, 12 px wide each, on the filled part
        let mut marked = plain.clone();
        paint(&mut marked, at(&b, 0.03), at(&b, 0.03) + 12, MARK);
        paint(&mut marked, at(&b, 0.08), at(&b, 0.08) + 12, MARK);
        let got = read(&marked, &b);
        assert!((got - clean).abs() < 0.002, "with the marks on it: {got}, without {clean}");
    }
}

#[test]
fn the_fills_real_end_is_still_its_end() {
    let b = bar();
    // the fill ends before a mark: the mark stands on the empty track, and nothing of the fill comes after it
    let mut im = frame(&b, 0.05);
    paint(&mut im, at(&b, 0.08), at(&b, 0.08) + 12, MARK);
    let got = read(&im, &b);
    assert!((got - 0.05).abs() < 0.004, "a fill that ends before the mark: {got}");
    // a hit's lighter trail after the fill is not the fill going on
    let mut im = frame(&b, 0.40);
    paint(&mut im, at(&b, 0.40), at(&b, 0.47), TRAIL);
    let got = read(&im, &b);
    assert!((got - 0.40).abs() < 0.004, "a hit's trail past the fill: {got}");
    // something wider than a mark on the bar (four thicknesses of another colour) is where the reading stops, as before
    let mut im = frame(&b, 0.80);
    paint(&mut im, at(&b, 0.30), at(&b, 0.30) + 104, [240, 240, 240]);
    let got = read(&im, &b);
    assert!((got - 0.30).abs() < 0.004, "a wide other colour on the bar: {got}");
}

#[test]
fn a_mark_is_passed_beside_our_own_covered_label() {
    let b = bar();
    let mut im = frame(&b, 0.70);
    paint(&mut im, at(&b, 0.08), at(&b, 0.08) + 12, MARK);
    paint(&mut im, at(&b, 0.42), at(&b, 0.49), COVER); // our HP label, covered in the picture
    let got = read(&im, &b);
    assert!((got - 0.70).abs() < 0.006, "the fill's end past a mark and a covered label: {got}");
}

/// By hand, on real pictures of the bar (4K frames, the bar at its usual place; the app keeps some in its data
/// folder under logs\bar_samples): prints what each reads.
/// `ALOGS_BAR_SAMPLES=<folder> cargo test -p alogs-reader --release --test bar_marks -- --ignored --nocapture`
#[test]
#[ignore = "needs real pictures of the bar"]
fn real_bar_pictures() {
    let dir = std::env::var("ALOGS_BAR_SAMPLES").expect("set ALOGS_BAR_SAMPLES");
    let mut files: Vec<_> = std::fs::read_dir(&dir).expect("the folder").flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "png")).collect();
    files.sort();
    for (n, f) in files.iter().enumerate() {
        let Some(im) = Image::open_png(f) else { continue };
        let im = im.convert(Mode::Rgb);
        let k = im.width as f64 / 3840.0;
        let (lb, span) = (1226.0 * k, 1388.0 * k);
        let b = Bar {
            yc: (194.0 * k) as i64,
            left_bracket: lb,
            span,
            brackets_found: true,
            world: true,
            right_red: 0.0,
            track_left: (lb + TRACK.0 * span).round_ties_even() as i64,
            track_right: (lb + WORLD_TRACK_RIGHT * span).round_ties_even() as i64,
            thickness: (26.0 * k) as i64,
            frame: (im.width, im.height),
        };
        let (fill, colour, edge) = measure_fill_any(&im, &b, 70);
        println!("picture {n:2} ({}x{}): {:.2}% (fill colour {colour:?}, after it {edge:?})", im.width, im.height, fill * 100.0);
    }
}
